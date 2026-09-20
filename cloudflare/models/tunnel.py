# -*- coding: utf-8 -*-
# Copyright © HAMS project. AGPL-3.0-or-later.
import logging
from urllib.parse import urlparse

from odoo import models, fields, api, _
from odoo.exceptions import AccessError, UserError
from ..utils.cloudflare_api import (
    delete_cfd_tunnel,
    get_cfd_tunnel_token,
    list_cfd_tunnels,
    update_cfd_tunnel_configuration,
)
from ..utils.cloudflare_daemon import is_tunnel_daemon_running, start_tunnel_daemon

_logger = logging.getLogger(__name__)

# The pre-2026-09-19 single, system-wide "have we pushed routes yet" flag, kept
# only so `_migrate_global_provisioned_flag` can fold it into the per-tunnel
# `routes_provisioned` field exactly once. Nothing else reads or writes it.
LEGACY_PROVISIONED_PARAM = "cloudflare.tunnel.provisioned"
LEGACY_PROVISIONED_MIGRATED = "migrated-to-per-tunnel"


class CloudflareTunnel(models.Model):
    _name = "cloudflare.tunnel"
    _description = "Cloudflare Tunnel"

    cf_tunnel_id = fields.Char(string="Tunnel ID", readonly=True, required=True)
    name = fields.Char(string="Tunnel Name", readonly=True, required=True)
    status = fields.Char(string="Status", readonly=True)
    created_at = fields.Datetime(string="Created At", readonly=True)
    website_id = fields.Many2one(
        "website",
        string="Website",
        default=lambda self: self.env["website"].get_current_website().id,
        readonly=True,
    )
    route_ids = fields.One2many(
        "cloudflare.tunnel.route", "tunnel_id", string="Routing Table"
    )
    # Replaces the single system-wide `cloudflare.tunnel.provisioned`
    # ir.config_parameter, which could not say anything per tunnel and so was
    # meaningless the moment a server fronted a second website. Kept on the
    # record it describes: no whitelist entry, no key-naming scheme, and it
    # disappears with the tunnel. It holds no secret -- just "have this
    # tunnel's ingress routes been pushed to Cloudflare at least once".
    routes_provisioned = fields.Boolean(
        string="Routes Provisioned",
        default=False,
        readonly=True,
        help="Set once this tunnel's ingress configuration has been pushed to "
        "Cloudflare. Cleared value means the next daemon start will push it "
        "(and retry on every later start until it succeeds).",
    )

    # [@ANCHOR: cloudflare:COMM_check_tunnel_caller_authorized]
    def _check_tunnel_caller_authorized(self):
        # Bug fix (bug-hunt, review_tier 1, 2026-09-09): same shape as
        # cloudflare.config.manager's own _check_waf_caller_authorized --
        # action_sync_tunnels is a public (non-underscore-prefixed)
        # @api.model method, directly dispatchable via /web/dataset/call_kw
        # by any authenticated session, including the public website user
        # (base.group_public has read on `website`, the very first model
        # this action queries). It then calls _sync_tunnels_for_website()
        # for every website, which elevates to the
        # cloudflare.user_cloudflare_tunnel service account BEFORE ever
        # touching cloudflare.tunnel's own (much stricter) ACL -- so an
        # anonymous site visitor could trigger a real Cloudflare API call
        # (list_cfd_tunnels) and real local cloudflare.tunnel writes for
        # every website in the system, with zero permission check. Gating
        # here, in the actual RPC entry point, closes that the same way
        # the WAF actions were closed.
        if not (
            self.env.user.has_group("cloudflare.group_cloudflare_tunnel")
            or self.env.user.has_group("base.group_system")
            or self.env.user.is_service_account
        ):
            raise AccessError(
                _("You are not authorized to manage Cloudflare Tunnels.")
            )

    # [@ANCHOR: cloudflare:COMM_tunnel_action_push_configuration]
    def action_push_configuration(self):
        # Pre-fetched once outside the loop below: the same global routes
        # (tunnel_id=False) apply to every tunnel in self, so searching for
        # them inside the loop would re-run the identical query once per
        # tunnel.
        global_routes = self.env["cloudflare.tunnel.route"].search([("tunnel_id", "=", False)], limit=10000)
        for tunnel in self:
            token, _zone = tunnel.website_id._get_cloudflare_credentials()
            account_id = tunnel.website_id.cloudflare_account_id

            if not token or not account_id:
                raise UserError(
                    _("Missing Cloudflare API Token or Account ID for the website.")
                )

            all_routes = tunnel.route_ids | global_routes

            ingress = []
            for route in all_routes.sorted('sequence'):
                rule = {"service": route.service_url}
                if route.hostname:
                    rule["hostname"] = route.hostname
                if route.path:
                    rule["path"] = route.path
                ingress.append(rule)
            
            # Add SSH route. cloudflared runs on the same host as the SSH
            # daemon and Odoo HTTP server it fronts, so localhost is the
            # real, correct proxy target here -- not a container-to-
            # container networking mistake.
            if tunnel.website_id.domain:
                parsed = urlparse(tunnel.website_id.domain)
                hostname = parsed.netloc or parsed.path
                if hostname:
                    ingress.append({"hostname": f"ssh.{hostname}", "service": "ssh://localhost:22"})  # burn-ignore-cloudflared-ingress

            # Catch-all required by Cloudflare
            ingress.append({"service": "http://localhost:8069"})  # burn-ignore-cloudflared-ingress

            payload = {"config": {"ingress": ingress}}
            success, msg = update_cfd_tunnel_configuration(
                account_id, token, tunnel.cf_tunnel_id, payload
            )
            if not success:
                raise UserError(_("Failed to push configuration: %s") % msg)

        # Bug fix (bug-hunt, review_tier 1, 2026-09-09): this notification
        # return used to live INSIDE the `for tunnel in self:` loop above,
        # at the same indent as the `payload =`/`update_cfd_tunnel_
        # configuration(...)` lines -- so calling this action on more than
        # one selected tunnel (the normal multi-select list-view case)
        # pushed configuration to only the first tunnel in the batch and
        # returned immediately, silently skipping every other tunnel in
        # `self` while still showing "Successfully pushed configuration"
        # as if the whole batch succeeded. Moved outside the loop so every
        # tunnel in `self` is actually processed before the one summary
        # notification is returned.
        # Simple notification since mail.thread isn't used
        return {
            "type": "ir.actions.client",
            "tag": "display_notification",
            "params": {
                "title": _("Success"),
                "message": _("Successfully pushed configuration to Cloudflare."),
                "type": "success",
                "sticky": False,
            },
        }

    def action_delete_tunnel(self):
        # [@ANCHOR: COMM_cf_delete_tunnel]

        # # Verified by [@ANCHOR: COMM_test_cf_delete_tunnel]
        # Bug fix (bug-hunt, review_tier 1, 2026-09-09), corrected same
        # day after an advisor review caught the first version of this
        # fix was transactionally inert: this used to collect every
        # successfully-remote-deleted tunnel into `tunnels_to_unlink` and
        # unlink them all in one batch AFTER the loop, with a `raise
        # UserError` on any tunnel's failure INSIDE the loop -- so a later
        # tunnel's failure aborted before that batched unlink() ever ran.
        # A first fix attempt moved the unlink() to run immediately after
        # each success, still inside the loop, still followed by `raise
        # UserError` on a later failure -- but a single Odoo RPC request
        # is one DB transaction that only commits at the very end; an
        # uncaught exception ANYWHERE in the request rolls the whole
        # transaction back, including every unlink() already executed
        # earlier in the same Python call, regardless of when in the
        # function they ran. That first fix was therefore transactionally
        # identical to the original bug -- verified by re-reading Odoo's
        # own request-dispatch/commit model, not assumed. The real fix:
        # stop raising when a partial success has already happened, since
        # raising is what discards it. Failures are now collected instead
        # of raised immediately; each successful remote delete is unlinked
        # locally as it happens; and only THEN does the method decide how
        # to report failures:
        failures = []
        for tunnel in self:
            token, _zone = tunnel.website_id._get_cloudflare_credentials()
            account_id = tunnel.website_id.cloudflare_account_id

            if not token or not account_id:
                failures.append(
                    _("%s: missing Cloudflare API Token or Account ID for the website.")
                    % tunnel.display_name
                )
                continue

            success, msg = delete_cfd_tunnel(account_id, token, tunnel.cf_tunnel_id)
            if success:
                # ADR-0001: Headless Mutation Context
                tunnel.unlink()
            else:
                failures.append(_("%s: %s") % (tunnel.display_name, msg))

        if not failures:
            return

        if len(failures) == len(self):
            # Nothing in this batch succeeded -- there is no already-
            # committed-in-this-request local unlink a rollback could
            # discard, so raising here is both safe and preserves this
            # action's original single-tunnel UX (a hard, modal error).
            raise UserError("\n".join(failures))

        # A MIX of success and failure: raising here would roll back the
        # whole transaction, including the tunnel(s) already unlinked
        # above in this same request -- re-orphaning them against their
        # already-deleted Cloudflare state, which is the exact bug this
        # fix exists to close. Report the failures as a non-fatal warning
        # instead, so the successful deletes actually commit when the
        # request finishes.
        return {
            "type": "ir.actions.client",
            "tag": "display_notification",
            "params": {
                "title": _("Some tunnels could not be deleted"),
                "message": "\n".join(failures),
                "type": "warning",
                "sticky": True,
            },
        }

    @api.model
    def action_sync_tunnels(self):
        # [@ANCHOR: COMM_cf_sync_tunnels]

        # # Verified by [@ANCHOR: COMM_test_cf_sync_tunnels]
        self._check_tunnel_caller_authorized()
        websites = self.env["website"].search([], limit=1000)
        for website in websites:
            # We sync synchronously because this is called via cron or manually, and we don't have queue_job.
            # Bug-hunt fix, 2026-09-11: an uncaught exception from any one website's sync (a
            # Cloudflare API failure, a malformed response, a DB constraint violation) used to
            # propagate straight out of this loop, aborting the sync for every OTHER website in
            # the same cron/manual run -- a transient failure on one customer's tunnels silently
            # starved every other website's sync until the next scheduled run. Isolating each
            # website's sync in its own savepoint (not just a bare try/except) matters
            # specifically for a DB-level failure: once a query raises inside a transaction,
            # Postgres marks the whole transaction aborted and refuses every further query on
            # that cursor until a rollback -- a bare try/except would stop the traceback there,
            # but every subsequent website's ORM calls would then also fail with "current
            # transaction is aborted". A savepoint rolls back only this website's own partial
            # writes, leaving the cursor usable for the rest of the loop.
            try:
                with self.env.cr.savepoint():
                    self._sync_tunnels_for_website(website.id)
            except Exception:  # audit-ignore-catch-all
                _logger.exception(
                    "Cloudflare tunnel sync failed for website %s (id=%s); continuing with "
                    "the remaining websites.",
                    website.name,
                    website.id,
                )

        return {
            "type": "ir.actions.client",
            "tag": "display_notification",
            "params": {
                "title": _("Success"),
                "message": _("Tunnels sync queued successfully."),
                "type": "success",
                "sticky": False,
            },
        }

    @api.model
    # [@ANCHOR: cloudflare:COMM_sync_tunnels_for_website]
    def _sync_tunnels_for_website(self, website_id):
        svc_uid = self.env["zero_sudo.security.utils"]._get_service_uid(
            "cloudflare.user_cloudflare_tunnel"
        )
        self = self.with_user(svc_uid)

        website = self.env["website"].browse(website_id)
        if not website.exists():
            return

        token, _zone = website._get_cloudflare_credentials()
        account_id = website.cloudflare_account_id

        if not token or not account_id:
            return

        tunnels = list_cfd_tunnels(account_id, token)
        existing_tunnels = {
            t.cf_tunnel_id: t
            for t in self.env["cloudflare.tunnel"].search(
                [("website_id", "=", website.id)], limit=10000
            )
        }

        tunnels_to_create = []
        for t in tunnels:
            tunnel_id = t.get("id")

            created_at_raw = t.get("created_at", "")
            created_at = False
            if created_at_raw:
                created_at = created_at_raw[:19].replace("T", " ")

            vals = {
                "cf_tunnel_id": tunnel_id,
                "name": t.get("name"),
                "status": t.get("status"),
                "created_at": created_at,
                "website_id": website.id,
            }

            existing = existing_tunnels.get(tunnel_id)
            if existing:
                existing.write(vals)
            else:
                tunnels_to_create.append(vals)

        if tunnels_to_create:
            self.env["cloudflare.tunnel"].create(tunnels_to_create)

    @api.model
    def _migrate_global_provisioned_flag(self):
        # [@ANCHOR: cloudflare:COMM_migrate_global_provisioned_flag]

        """Fold the old single global `provisioned` flag into the per-tunnel field.

        `cloudflare.tunnel.provisioned` was one system-wide ir.config_parameter,
        which could only ever mean one thing, because the code that set it only
        ever handled one tunnel: "the tunnel that `search([], limit=1)` picked
        has had its routes pushed". `search([], limit=1)` with no `order` on a
        model with no `_order` is the lowest id, so that is exactly the record
        the flag is folded into here -- NOT every existing tunnel, which would
        wrongly mark tunnels that were never provisioned and skip their first
        route push forever.

        Idempotent, and safe to call on every cron tick: the parameter is
        rewritten to a sentinel afterwards, so an already-migrated install does
        nothing and an install that never had the flag does nothing either. The
        parameter is left in place rather than deleted so that a rollback to the
        previous code still finds a truthy value and does not re-provision.

        (The parameter's zero-sudo read/write whitelist entries stay as they
        are: this flag is the one key this migration needs, and nothing new was
        added to that whitelist.)
        """
        # .sudo() is forbidden on this platform -- cloudflare.tunnel.provisioned
        # is a plain progress flag (same category as the already-whitelisted
        # cloudflare.last_static_mtime), so it goes through the same
        # Zero-Sudo-vetted _get_system_param()/_set_system_param() helper
        # config_manager.py already uses for that sibling flag.
        utils = self.env["zero_sudo.security.utils"]
        # `redis_bypass_cache` on the READ specifically, and it is what makes
        # this function's idempotence real rather than nominal.
        # `_get_system_param` is `@distributed_cache()`'d, and writing the
        # parameter through `_set_system_param` does NOT evict that cache
        # entry -- so a plain read would keep returning the pre-migration
        # value for as long as the entry lives, and this migration would
        # "run" again on every cron tick, five minutes apart, forever. Caught
        # by `test_06_the_migration_runs_once_and_is_a_no_op_afterwards`
        # failing on exactly that, not reasoned about in advance.
        # `redis_bypass_cache` is the decorator's own documented escape hatch
        # and is already used this way in production code
        # (`distributed_redis_cache/redis_pool.py`). Only this one read needs
        # it: nothing else in the codebase reads this parameter any more.
        raw = utils.with_context(redis_bypass_cache=True)._get_system_param(
            LEGACY_PROVISIONED_PARAM
        )
        if not raw or raw == LEGACY_PROVISIONED_MIGRATED:
            return False

        legacy_tunnel = self.env["cloudflare.tunnel"].search(
            [], order="id asc", limit=1
        )
        if legacy_tunnel and not legacy_tunnel.routes_provisioned:
            legacy_tunnel.routes_provisioned = True
            _logger.info(
                "Cloudflare: migrated the global tunnel-provisioned flag onto "
                "tunnel %s (the record the old single-tunnel code operated on).",
                legacy_tunnel.cf_tunnel_id,
            )
        utils._set_system_param(LEGACY_PROVISIONED_PARAM, LEGACY_PROVISIONED_MIGRATED)
        return True

    def _ensure_one_tunnel_running(self, tunnel):
        # [@ANCHOR: cloudflare:COMM_ensure_one_tunnel_running]

        """Bring exactly one tunnel up, returning a one-word outcome for the summary.

        Returns "already_running", "skipped", "started" or "failed". Never
        raises for an ordinary Cloudflare problem -- `action_ensure_tunnel_running`
        depends on one tunnel's bad day not taking the other websites down with
        it.
        """
        if not tunnel.cf_tunnel_id:
            return "skipped"

        # A tunnel that is already up AND already provisioned needs nothing:
        # skipping here is what stops the five-minute cron making a Cloudflare
        # API call per tunnel per tick on a server fronting a dozen sites.
        if tunnel.routes_provisioned and is_tunnel_daemon_running(tunnel.cf_tunnel_id):
            return "already_running"

        token, _zone = tunnel.website_id._get_cloudflare_credentials()
        account_id = tunnel.website_id.cloudflare_account_id

        if not token or not account_id:
            _logger.info(
                "Cloudflare tunnel %s: its website has no API token or account id "
                "configured; skipping it and continuing with the other tunnels.",
                tunnel.cf_tunnel_id,
            )
            return "skipped"

        success, result = get_cfd_tunnel_token(account_id, token, tunnel.cf_tunnel_id)
        if not success:
            # `result` is the ERROR MESSAGE on failure and the RUN TOKEN on
            # success -- the (bool, str) convention every cloudflare_api helper
            # uses. Logging it is therefore only ever safe inside this
            # `not success` branch, and the empty-token branch below logs a
            # fixed string instead of `result` for exactly that reason.
            _logger.error(
                "Cloudflare tunnel %s: could not fetch its run token: %s",
                tunnel.cf_tunnel_id,
                result,
            )
            return "failed"
        if not result:
            _logger.error(
                "Cloudflare tunnel %s: Cloudflare reported success but returned "
                "an empty run token.",
                tunnel.cf_tunnel_id,
            )
            return "failed"

        if not tunnel.routes_provisioned:
            try:
                tunnel.action_push_configuration()
                tunnel.routes_provisioned = True
            # A Cloudflare API failure while pushing routes must not
            # prevent the tunnel daemon itself from starting -- SSH/
            # basic connectivity should stay up while provisioning
            # retries on the next call. The flag stays False precisely so
            # that the next call does retry.
            except Exception as e:  # audit-ignore-catch-all: Tested by [@ANCHOR: cloudflare_tunnel_push_config_catch_all]  # fmt: skip
                _logger.error(
                    "Failed to provision routes on initial start of tunnel %s: %s",
                    tunnel.cf_tunnel_id,
                    e,
                )

        # start_tunnel_daemon is itself idempotent per key, but asking first
        # keeps the summary honest about what this call actually did.
        if is_tunnel_daemon_running(tunnel.cf_tunnel_id):
            return "already_running"
        start_tunnel_daemon(result, tunnel_key=tunnel.cf_tunnel_id)
        return "started"

    @api.model
    def action_ensure_tunnel_running(self):
        # [@ANCHOR: cloudflare:COMM_ensure_tunnel_running]

        """Ensure EVERY tunnel that has credentials is running, not just the first.

        This used to be `search([], limit=1)`: on a server with more than one
        `cloudflare.tunnel` record it silently operated on the lowest-id one and
        ignored every other website's tunnel, with no error and no log. Bruce's
        answer of 2026-09-19 (see
        `hams_com/night_shift_questions/answered/
        cloudflare-tunnel-ensure-running-multi-tunnel-scope-ec6882e6.md`) settled
        the scope: "It ends up that one server fronting multiple web sites is
        the common case ... So, make that work correctly."

        One `cloudflared` daemon per tunnel, each tracked independently by its
        Cloudflare tunnel id, so that an already-running tunnel is never started
        twice, one that died IS restarted, and stopping or failing one never
        touches another's daemon.

        Each tunnel runs inside its own savepoint, for the same reason
        `action_sync_tunnels` does: once a query raises inside a transaction,
        PostgreSQL refuses every further query on that cursor until a rollback,
        so a bare try/except would leave every LATER tunnel failing with
        "current transaction is aborted" instead of isolating the one that
        broke.

        Returns a counts summary (also logged), so a cron run says what it did
        across all the tunnels rather than silently doing one.
        """
        # This search stays the FIRST thing this method does, deliberately.
        # cloudflare.tunnel's ACL is what gates this whole (publicly
        # dispatchable, @api.model) entry point for any caller outside
        # cloudflare.group_cloudflare_tunnel / base.group_system, so nothing --
        # not even the migration's whitelisted ir.config_parameter read -- may
        # run ahead of it.
        summary = {"started": 0, "already_running": 0, "skipped": 0, "failed": 0}
        tunnels = self.env["cloudflare.tunnel"].search([], limit=10000)
        if not tunnels:
            # Returning here is a second half of the gate above, not just an
            # early-out for a trivial case: `search([])` on a model the caller
            # has no ACL row for raises only once it has records to check, so
            # on an install with NO tunnels the search alone does not stop an
            # unauthorized caller. Without this, such a caller would go on to
            # reach the migration's whitelisted ir.config_parameter read. With
            # it, a caller who cannot see tunnels can reach nothing at all.
            return summary

        self._migrate_global_provisioned_flag()

        for tunnel in tunnels:
            try:
                with self.env.cr.savepoint():
                    summary[self._ensure_one_tunnel_running(tunnel)] += 1
            except Exception:  # audit-ignore-catch-all
                summary["failed"] += 1
                _logger.exception(
                    "Cloudflare: ensuring tunnel %s is running failed; continuing "
                    "with the remaining tunnels.",
                    tunnel.cf_tunnel_id,
                )

        _logger.info(
            "Cloudflare: ensure-tunnel-running over %s tunnel(s) -- started %s, "
            "already running %s, skipped %s, failed %s.",
            len(tunnels),
            summary["started"],
            summary["already_running"],
            summary["skipped"],
            summary["failed"],
        )
        return summary
