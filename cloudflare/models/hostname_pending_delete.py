# -*- coding: utf-8 -*-
# Copyright © HAMS project. AGPL-3.0-or-later.
import logging
from datetime import timedelta

from odoo import models, fields, api, _
from odoo.exceptions import AccessError
from ..utils import cloudflare_api as cf_utils

_logger = logging.getLogger(__name__)

# After this many failed attempts the record stops retrying and stays visible
# for an admin instead. Eight attempts with the backoff below spans roughly a
# day and a half: long enough to ride out a Cloudflare incident or a rate
# limit, short enough that a genuinely dead credential stops generating
# traffic rather than retrying until somebody notices.
MAX_DELETE_ATTEMPTS = 8

# Bounded exponential backoff, in minutes, indexed by attempts already made.
# Capped rather than doubling forever, because an un-cleared record should keep
# checking occasionally (a revoked token can be replaced) without ever becoming
# a busy loop.
BACKOFF_MINUTES = [5, 10, 20, 40, 80, 160, 320, 720]


class CloudflareHostnamePendingDelete(models.Model):
    """A custom hostname Cloudflare would not delete, kept so it can be retried.

    `edge.routing.domain.unlink()` deletes the local row whatever Cloudflare
    says -- blocking it would make a domain undeletable whenever a token is
    revoked for good, which is worse than the leak it prevents. Bruce's
    decision (2026-09-19, `night_shift_questions/answered/
    cloudflare-hostname-delete-failure-block-or-proceed-05a5144b.md`, "3 sounds
    good to me") is the third option on that question: **proceed with the local
    delete, but keep a retry record**, so the orphaned hostname on Cloudflare's
    zone has something pointing at it rather than only a log line nothing ever
    revisits.

    **No credential is ever stored here.** The record holds the Cloudflare
    custom-hostname id, the domain name, the website the credentials come FROM,
    Cloudflare's own error text and some counters. The API token is fetched
    from the website at retry time, exactly as the original delete did.
    """

    _name = "cloudflare.hostname.pending.delete"
    _description = "Cloudflare Custom Hostname Pending Delete"
    _order = "state, next_attempt_at, id"

    name = fields.Char(string="Name", compute="_compute_name", readonly=True)
    cf_hostname_id = fields.Char(
        string="Cloudflare Hostname ID", required=True, readonly=True, index=True
    )
    domain_name = fields.Char(string="Domain", required=True, readonly=True)
    website_id = fields.Many2one(
        "website",
        string="Website",
        ondelete="set null",
        readonly=True,
        help="The website whose Cloudflare credentials and zone this hostname "
        "belongs to. A pointer only -- no token is stored on this record.",
    )
    last_error = fields.Char(string="Last Cloudflare Error", readonly=True)
    attempt_count = fields.Integer(string="Attempts", default=0, readonly=True)
    last_attempt_at = fields.Datetime(string="Last Attempt", readonly=True)
    next_attempt_at = fields.Datetime(string="Next Attempt", readonly=True, index=True)
    state = fields.Selection(
        [
            ("pending", "Pending"),
            ("done", "Done"),
            ("abandoned", "Abandoned"),
        ],
        default="pending",
        required=True,
        index=True,
        readonly=True,
    )

    # [@ANCHOR: cloudflare:COMM_pending_delete_compute_name]
    @api.depends("domain_name", "cf_hostname_id")
    def _compute_name(self):
        for record in self:
            record.name = "%s (%s)" % (
                record.domain_name or _("Unknown domain"),
                record.cf_hostname_id or "-",
            )

    @api.model
    # [@ANCHOR: cloudflare:COMM_pending_delete_next_attempt_at]
    def _next_attempt_at(self, attempts_made, now=None):
        """When the retry after `attempts_made` failures becomes due.

        Reads the bounded `BACKOFF_MINUTES` table, clamping at both ends rather
        than indexing off it, so a record that somehow carries a count outside
        the table still produces a sane datetime instead of raising inside a
        cron and taking every other pending record down with it.

        `now` is injectable so that a caller -- in practice a test -- can pass
        ONE base time to several calls and compare the results exactly. Reading
        the clock internally on every call would make "the backoff stops
        growing" unassertable: two calls a few microseconds apart differ by
        that drift, so an equality check would be measuring the clock rather
        than the clamp.
        """
        index = min(max(attempts_made, 0), len(BACKOFF_MINUTES) - 1)
        base = now or fields.Datetime.now()
        return base + timedelta(minutes=BACKOFF_MINUTES[index])

    @api.model
    # [@ANCHOR: cloudflare:COMM_record_failed_hostname_delete]
    def _record_failed_delete(self, domain_name, cf_hostname_id, website, error):
        """Record (or refresh) one hostname Cloudflare refused to delete.

        Keyed on `cf_hostname_id`, which is Cloudflare's own handle for one
        custom hostname: a second failure for the same id is the same leak, not
        a new one, and must not accumulate duplicate rows the cron would then
        retry in parallel against the same remote object. An existing row in
        ANY state is reused and reopened -- including a `done` or `abandoned`
        one, because a delete that just failed is direct evidence the hostname
        still exists whatever that row previously concluded.
        """
        if not cf_hostname_id:
            return self.browse()
        vals = {
            "domain_name": domain_name,
            "website_id": website.id if website else False,
            # Truncated, and it is Cloudflare's own message rather than
            # anything this side composed, so it cannot carry a token.
            "last_error": (error or "")[:500],
            "state": "pending",
        }
        # Deliberately NOT `.sudo()`-ed (forbidden on this platform): this runs
        # with whatever identity the caller already holds -- the admin doing
        # the `unlink()` for a fresh failure, the cron's service account for a
        # retry.
        existing = self.search([("cf_hostname_id", "=", cf_hostname_id)], limit=1)
        if existing:
            existing.write(
                dict(
                    vals,
                    next_attempt_at=self._next_attempt_at(existing.attempt_count),
                )
            )
            return existing
        vals.update(
            {
                "cf_hostname_id": cf_hostname_id,
                "next_attempt_at": self._next_attempt_at(0),
            }
        )
        return self.create(vals)

    # [@ANCHOR: cloudflare:COMM_pending_delete_attempt]
    def _attempt_delete(self):
        """Try the Cloudflare delete once for each record in `self`.

        Returns a `{outcome: count}` summary. Each record is handled inside its
        own savepoint for the same reason `action_sync_tunnels` uses one: a
        query that raises inside a transaction leaves the cursor unusable until
        rollback, so without it one bad record would take every later record in
        the same cron run down with it.
        """
        summary = {"done": 0, "retrying": 0, "abandoned": 0, "skipped": 0}
        for record in self:
            try:
                with self.env.cr.savepoint():
                    summary[record._attempt_one_delete()] += 1
            except Exception:  # audit-ignore-catch-all
                summary["retrying"] += 1
                _logger.exception(
                    "Cloudflare pending hostname delete %s raised; it stays "
                    "pending and the remaining records continue.",
                    record.cf_hostname_id,
                )
        return summary

    # [@ANCHOR: cloudflare:COMM_pending_delete_attempt_one]
    def _attempt_one_delete(self):
        """One record, one Cloudflare call; returns a one-word outcome.

        `"done"` when Cloudflare confirms the delete OR reports the hostname is
        already gone -- `delete_custom_hostname` reports a 404 as success
        precisely so that "somebody removed it in the dashboard" closes this
        record instead of retrying forever against something that no longer
        exists.
        """
        self.ensure_one()
        if not self.website_id:
            self._fail_attempt(
                _("The website this hostname belonged to no longer exists.")
            )
            return "abandoned" if self.state == "abandoned" else "retrying"

        token, zone_id = self.website_id._get_cloudflare_credentials()
        if not token or not zone_id:
            self._fail_attempt(_("No Cloudflare API token or zone id configured."))
            return "abandoned" if self.state == "abandoned" else "retrying"

        success, message = cf_utils.delete_custom_hostname(
            self.cf_hostname_id, token, zone_id
        )
        if success:
            self.write(
                {
                    "state": "done",
                    "attempt_count": self.attempt_count + 1,
                    "last_attempt_at": fields.Datetime.now(),
                    "next_attempt_at": False,
                    "last_error": False,
                }
            )
            _logger.info(
                "Cloudflare custom hostname %s (%s) is gone; pending delete closed.",
                self.cf_hostname_id,
                self.domain_name,
            )
            return "done"

        self._fail_attempt(message)
        return "abandoned" if self.state == "abandoned" else "retrying"

    # [@ANCHOR: cloudflare:COMM_pending_delete_fail_attempt]
    def _fail_attempt(self, message):
        """Book one more failed attempt, abandoning the record at the cap.

        Abandoning does NOT delete the row: the whole point of this model is
        that an orphaned hostname on Cloudflare's zone stays visible to an
        admin, who can fix the credential and press Retry. It only stops the
        cron spending calls on it.
        """
        self.ensure_one()
        attempts = self.attempt_count + 1
        vals = {
            "attempt_count": attempts,
            "last_attempt_at": fields.Datetime.now(),
            "last_error": (message or "")[:500],
        }
        if attempts >= MAX_DELETE_ATTEMPTS:
            vals.update({"state": "abandoned", "next_attempt_at": False})
            _logger.warning(
                "Cloudflare custom hostname %s (%s) could not be deleted after "
                "%s attempts (%s); abandoning automatic retries. The hostname "
                "may still be active on the zone -- an admin can retry it from "
                "Cloudflare Edge > Pending Hostname Deletes.",
                self.cf_hostname_id,
                self.domain_name,
                attempts,
                message,
            )
        else:
            vals.update(
                {"state": "pending", "next_attempt_at": self._next_attempt_at(attempts)}
            )
        self.write(vals)

    @api.model
    # [@ANCHOR: cloudflare:COMM_cron_retry_pending_hostname_deletes]
    def _cron_retry_pending_deletes(self):
        """Retry every pending record whose backoff has elapsed."""
        due = self.search(
            [
                ("state", "=", "pending"),
                "|",
                ("next_attempt_at", "=", False),
                ("next_attempt_at", "<=", fields.Datetime.now()),
            ],
            limit=1000,
        )
        if not due:
            return {"done": 0, "retrying": 0, "abandoned": 0, "skipped": 0}
        summary = due._attempt_delete()
        _logger.info(
            "Cloudflare pending hostname deletes: %s due -- %s done, %s still "
            "retrying, %s abandoned.",
            len(due),
            summary["done"],
            summary["retrying"],
            summary["abandoned"],
        )
        return summary

    # [@ANCHOR: cloudflare:COMM_pending_delete_action_retry_now]
    def action_retry_now(self):
        """Admin button: try the selected records immediately.

        Works on an `abandoned` record too -- that is the whole reason
        abandoning leaves the row in place rather than deleting it.
        """
        self._check_pending_delete_admin()
        self._attempt_delete()
        return True

    # [@ANCHOR: cloudflare:COMM_pending_delete_action_abandon]
    def action_mark_abandoned(self):
        """Admin button: stop retrying, keep the record visible."""
        self._check_pending_delete_admin()
        self.write({"state": "abandoned", "next_attempt_at": False})
        return True

    # [@ANCHOR: cloudflare:COMM_pending_delete_check_admin]
    def _check_pending_delete_admin(self):
        """Gate the two buttons on real admin rights, before any network call.

        Same shape as `domain.py`'s `action_sync_ssl_status`: these are public
        (non-underscore-prefixed) methods, so they are dispatchable over
        `/web/dataset/call_kw` by anyone who can name a record id, and
        `action_retry_now` makes a real, credentialed Cloudflare API call. The
        model's ACL already grants ordinary users nothing, but the check is
        made explicitly so the network call is gated by the same permission the
        write needs rather than only by the ACL of the first field read.
        """
        if not (
            self.env.user.has_group("base.group_system")
            or self.env.user.is_service_account
        ):
            raise AccessError(
                _(
                    "You are not authorized to manage pending Cloudflare "
                    "hostname deletes."
                )
            )
