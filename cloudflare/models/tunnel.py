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
from ..utils.cloudflare_daemon import start_tunnel_daemon

_logger = logging.getLogger(__name__)


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
            self._sync_tunnels_for_website(website.id)

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
    def action_ensure_tunnel_running(self):
        # Find the primary tunnel for the current website
        # In a single-server setup, we just pick the first tunnel available.
        tunnel = self.env["cloudflare.tunnel"].search([], limit=1)
        if not tunnel:
            return

        token, _zone = tunnel.website_id._get_cloudflare_credentials()
        account_id = tunnel.website_id.cloudflare_account_id

        if not token or not account_id:
            return

        success, tunnel_token = get_cfd_tunnel_token(account_id, token, tunnel.cf_tunnel_id)
        if success and tunnel_token:
            # .sudo() is forbidden on this platform -- cloudflare.tunnel.provisioned
            # is a plain progress flag (same category as the already-whitelisted
            # cloudflare.last_static_mtime), so it goes through the same
            # Zero-Sudo-vetted _get_system_param()/_set_system_param() helper
            # config_manager.py already uses for that sibling flag.
            utils = self.env["zero_sudo.security.utils"]
            if not utils._get_system_param('cloudflare.tunnel.provisioned'):
                try:
                    tunnel.action_push_configuration()
                    utils._set_system_param('cloudflare.tunnel.provisioned', 'True')
                # A Cloudflare API failure while pushing routes must not
                # prevent the tunnel daemon itself from starting -- SSH/
                # basic connectivity should stay up while provisioning
                # retries on the next call.
                except Exception as e:  # audit-ignore-catch-all: Tested by [@ANCHOR: cloudflare_tunnel_push_config_catch_all]  # fmt: skip
                    _logger.error("Failed to provision routes on initial tunnel start: %s", e)
            start_tunnel_daemon(tunnel_token)
