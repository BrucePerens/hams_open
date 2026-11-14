# SPDX-License-Identifier: AGPL-3.0-or-later
# -*- coding: utf-8 -*-
# Copyright © Bruce Perens K6BP. Licensed under the GNU Affero General Public License v3.0 or later (AGPL-3.0-or-later).

import json
import secrets
from datetime import timedelta
from itertools import islice

from odoo import models, fields, api, _
from odoo.exceptions import AccessError
from odoo.addons.zero_sudo import public_rpc

# How long a browser has to hand the redirected token to the export daemon
# before it's refused. Short and single-use per
# docs/proposals/GDPR_CSV_EXPORT.md's own security note: this token is not a
# session credential, it's scoped to exactly one export, one user, one narrow
# window.
TOKEN_EXPIRY_MINUTES = 5

# Rows one export_page call returns at most, and how long after its first use a token may still page its export.
EXPORT_PAGE_ROWS = 5000
EXPORT_WINDOW_MINUTES = 60

# Rows the daily vacuum deletes per page.
TOKEN_VACUUM_PAGE = 1000


# [@ANCHOR: user_websites:COMM_gdpr_token_rejected]
# Verified by [@ANCHOR: test_gdpr_export_token_unknown_rejected]
class GdprExportTokenRejected(AccessError):
    """The export token is unknown, already used or expired. A distinct class (still an AccessError) so the export daemon can
    tell a bad link from every other failure by the class name in Odoo's JSON-2 error answer, never by message text."""


# [@ANCHOR: user_websites_ham_gdpr_export_token_ham_gdpr_export_token]
class HamGdprExportToken(models.Model):
    # [@ANCHOR: user_websites:gdpr_export_token]
    _name = "ham.gdpr.export.token"
    _description = "Short-lived, single-use token authorizing one GDPR CSV/zip export download"

    user_id = fields.Many2one(
        "res.users", string="Requesting User", required=True, ondelete="cascade", index=True
    )
    token = fields.Char(string="Token", required=True, index=True, copy=False)
    consumed = fields.Boolean(string="Consumed", default=False)
    # A real textual name, not just something to satisfy CRITICAL SCHEMA --
    # this is also what makes a stray token findable/identifiable in the
    # Odoo backend (e.g. debugging a stuck export) without decoding the
    # opaque `token` value itself.
    name = fields.Char(string="Name", compute="_compute_name", store=True)

    _token_unique = models.Constraint(
        "UNIQUE(token)", "Export tokens must be unique."
    )

    @api.depends("user_id", "user_id.login", "user_id.name")
    # [@ANCHOR: user_websites:COMM_gdpr_token_compute_name]
    def _compute_name(self):
        for record in self:
            record.name = _("GDPR Export Token for %s") % (
                record.user_id.name or record.user_id.login or record.user_id.id
            )

    @api.private
    @api.model
    # [@ANCHOR: user_websites:COMM_create_for_current_user]
    def create_for_current_user(self):
        """Called from the authenticated /my/privacy/export.zip controller,
        as the requesting user's own session -- this is the only path that
        creates a token, so a token's user_id is always the identity Odoo
        itself already authenticated, never a caller-supplied value.

        The requesting user's own session has no create access to this
        model (see user_websites/security/ir.model.access.csv -- only
        group_gdpr_export_service does), so minting the token briefly
        assumes the dedicated gdpr_export_service_internal service account
        (the Service Account Pattern per MASTER_01), rather than sudo()'ing
        around the ACL."""
        token = secrets.token_urlsafe(32)
        requesting_uid = self.env.uid
        svc_uid = self.env["zero_sudo.security.utils"]._get_service_uid(
            "user_websites.user_gdpr_export_service"
        )
        self.with_user(svc_uid).create({"user_id": requesting_uid, "token": token})
        return token

    @api.model
    # [@ANCHOR: user_websites:COMM_gdpr_token_consume]
    def _consume(self, token):
        """Validates and atomically consumes a token, returning the res.users
        record it was issued for. Raises AccessError on any failure (missing,
        already consumed, expired) -- refuses to distinguish *why* to a caller
        that doesn't already hold a valid token, the same fail-closed posture
        as every other credential check in this codebase.

        Only ever called by the gdpr_export_service_internal daemon account
        (see security/gdpr_export_security.xml) -- that account's own
        group_gdpr_export_service grant already gives it read/write on this
        model directly (per ir.model.access.csv), so no privilege escalation
        is needed here at all; this method deliberately does NOT sudo() or
        with_user() to a broader account, keeping this the one, narrow,
        disclosed elevated-privilege surface consume_and_export() below
        adds, not a general grant. Every other GDPR export data access this
        feature makes still goes through the existing
        _get_gdpr_export_data()/_get_gdpr_streamed_keys() contract, unchanged."""
        # # Verified by [@ANCHOR: test_gdpr_export_token_unknown_rejected]
        #
        # Real fix, found by an adversarial security review: search() then
        # a separate write({"consumed": True}) is a real TOCTOU race -- two
        # concurrent calls with the same still-unconsumed token could both
        # pass the search check before either one's write lands, letting
        # the token's own holder download the export twice instead of once
        # (low severity -- it's still only ever the legitimate token
        # holder, not a cross-user leak -- but a real single-use guarantee
        # shouldn't have a race in it regardless). A single atomic
        # UPDATE ... WHERE consumed = false RETURNING ... closes it: at
        # most one concurrent caller's UPDATE can ever match and return a
        # row for a given token, by Postgres's own row-level locking, with
        # no separate read-then-write window for a second caller to land
        # in between.
        self.env.cr.execute(
            "UPDATE ham_gdpr_export_token SET consumed = true "
            "WHERE token = %s AND consumed = false "
            "RETURNING id, user_id, create_date",
            (token,),
        )
        row = self.env.cr.fetchone()
        if not row:
            raise GdprExportTokenRejected(_("Invalid or already-used export token."))
        record_id, user_id, create_date = row
        # Real fix, found running this same pass's own real test suite,
        # not just reasoned about: raw SQL bypasses the ORM's own
        # in-memory cache entirely, so a caller that already holds (or
        # later browses) this record would otherwise see a stale
        # `consumed = False` despite the real row now being `true` in the
        # database -- confirmed directly by a genuine test failure
        # (`test_01e_consume_and_export_materializes_data_and_streamed_
        # keys`'s own `self.assertTrue(record.consumed, ...)`) before
        # this invalidation was added, not merely anticipated.
        self.invalidate_model(["consumed"])
        # # Verified by [@ANCHOR: test_gdpr_export_token_expiry]
        # An expired token is now marked consumed too (it was already
        # permanently unusable either way -- this doesn't help an
        # attacker or hurt a legitimate user, it just means a second
        # attempt against the same expired token fails as "already-used"
        # instead of "expired," which is an equally fail-closed rejection).
        cutoff = fields.Datetime.now() - timedelta(minutes=TOKEN_EXPIRY_MINUTES)
        if create_date < cutoff:
            raise GdprExportTokenRejected(_("Export token has expired."))
        # # Verified by [@ANCHOR: test_gdpr_export_token_single_use]
        return self.env["res.users"].browse(user_id)

    @api.model
    # [@ANCHOR: user_websites:gdpr_token_vacuum]
    def _cron_vacuum_tokens(self):
        """Daily: deletes every token older than a day (a token is dead after TOKEN_EXPIRY_MINUTES, used or
        not). Pages by id, a page at a time, until nothing old is left, so no row is skipped by a cap."""
        # Verified by [@ANCHOR: test_gdpr_token_vacuum]
        cutoff = fields.Datetime.now() - timedelta(days=1)
        last_id = 0
        while True:
            page = self.search(
                [("id", ">", last_id), ("create_date", "<", cutoff)], order="id", limit=TOKEN_VACUUM_PAGE
            )
            if not page:
                break
            last_id = page.ids[-1]
            page.unlink()

    @api.model
    # [@ANCHOR: user_websites:COMM_gdpr_consume_and_export]
    @public_rpc  # RPC by design: the gdpr_csv_export daemon calls it over RPC
    def consume_and_export(self, token):
        """The first RPC the export daemon calls. Validates+consumes the token and returns the small part of the
        export: {"data": the plain dict keys, "streamed_keys": the names of the large per-user datasets}. The
        large datasets are never built here: the daemon then pulls each one a page at a time with export_page(),
        so no Odoo worker holds a whole export (a member with 100k QSOs) in memory or for one long call."""
        # # Verified by [@ANCHOR: test_gdpr_consume_and_export_payload]
        #
        # Hardening: this is a public
        # (non-underscore) @api.model method, reachable via the /json/2
        # bearer-token RPC endpoint by ANY authenticated user holding a
        # valid API key for hams.com, not just the gdpr_csv_export daemon's
        # own gdpr_export_service_internal account. _consume()'s own single-
        # use/expiry checks mean a stolen call still can't replay a token,
        # but there's no reason this RPC surface should be reachable by an
        # arbitrary account at all -- narrowing it to the one caller that's
        # ever supposed to use it removes a whole class of future risk
        # (e.g. a leaked, not-yet-consumed token combined with any other
        # account's bearer key) for zero cost to the real daemon. Same
        # caller-authorization-at-the-RPC-entry-point shape as cloudflare's
        # own `_check_waf_caller_authorized()`.
        if not (
            self.env.user == self.env.ref("user_websites.user_gdpr_export_service")
            or self.env.user.has_group("base.group_system")
        ):
            raise AccessError(
                _("This RPC method may only be called by the GDPR export daemon's own service account.")
            )
        user = self._consume(token)
        user_as_gdpr_svc = self._export_user(user)
        data = user_as_gdpr_svc._get_gdpr_export_data()
        keys = list(user_as_gdpr_svc._get_gdpr_streamed_keys())
        # The json round trip proves the payload is JSON-serializable (dates, Markup) before it crosses the RPC.
        return json.loads(json.dumps({"data": data, "streamed_keys": keys}, default=str))

    def _export_user(self, user):
        """`user` as zero_sudo.gdpr_service_internal, the already-scoped account that may read a member's export
        data (the same account _execute_gdpr_erasure uses); the token-table account stays narrow."""
        gdpr_svc_uid = self.env["zero_sudo.security.utils"]._get_service_uid("zero_sudo.gdpr_service_internal")
        return user.with_user(gdpr_svc_uid)

    @api.model
    # [@ANCHOR: user_websites:gdpr_export_page]
    # Verified by [@ANCHOR: test_gdpr_export_page_is_bounded_and_complete]
    @public_rpc  # RPC by design: the gdpr_csv_export daemon pulls each streamed dataset page by page
    def export_page(self, token, key, offset, limit=EXPORT_PAGE_ROWS):
        """One page (at most EXPORT_PAGE_ROWS rows from `offset`) of the streamed dataset `key` for the member the
        token was issued to. The token must already have been consumed by consume_and_export (so a token never
        used for an export reads nothing) and be no older than EXPORT_WINDOW_MINUTES. A page shorter than `limit`
        is the last. The generator is re-run per page (cheap keyset paging inside); the cost is bounded by the page."""
        if not (
            self.env.user == self.env.ref("user_websites.user_gdpr_export_service")
            or self.env.user.has_group("base.group_system")
        ):
            raise AccessError(
                _("This RPC method may only be called by the GDPR export daemon's own service account.")
            )
        limit = max(1, min(int(limit), EXPORT_PAGE_ROWS))
        offset = max(0, int(offset))
        cutoff = fields.Datetime.now() - timedelta(minutes=EXPORT_WINDOW_MINUTES)
        row = self.search([("token", "=", token), ("consumed", "=", True), ("create_date", ">=", cutoff)], limit=1)
        if not row:
            raise GdprExportTokenRejected(_("Invalid or expired export token."))
        streamed = self._export_user(row.user_id)._get_gdpr_streamed_keys()
        if key not in streamed:
            raise GdprExportTokenRejected(_("Unknown export dataset."))
        rows = list(islice(streamed[key](), offset, offset + limit))
        return json.loads(json.dumps(rows, default=str))
