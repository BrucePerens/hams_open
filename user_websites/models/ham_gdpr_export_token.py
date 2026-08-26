# SPDX-License-Identifier: AGPL-3.0-or-later
# -*- coding: utf-8 -*-
# Copyright © Bruce Perens K6BP. Licensed under the GNU Affero General Public License v3.0 or later (AGPL-3.0-or-later).

import json
import secrets
from datetime import timedelta

from odoo import models, fields, api, _
from odoo.exceptions import AccessError

# How long a browser has to hand the redirected token to the export daemon
# before it's refused. Short and single-use per
# docs/proposals/GDPR_CSV_EXPORT.md's own security note: this token is not a
# session credential, it's scoped to exactly one export, one user, one narrow
# window.
TOKEN_EXPIRY_MINUTES = 5


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
    def _compute_name(self):
        for record in self:
            record.name = _("GDPR Export Token for %s") % (
                record.user_id.name or record.user_id.login or record.user_id.id
            )

    @api.model
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
        record = self.env[self._name].search(
            [("token", "=", token), ("consumed", "=", False)], limit=1
        )
        if not record:
            raise AccessError(_("Invalid or already-used export token."))
        # # Verified by [@ANCHOR: test_gdpr_export_token_expiry]
        cutoff = fields.Datetime.now() - timedelta(minutes=TOKEN_EXPIRY_MINUTES)
        if record.create_date < cutoff:
            raise AccessError(_("Export token has expired."))
        # Consume first, fetch after: a failure while building the export
        # payload below must not leave a still-valid, replayable token behind.
        # # Verified by [@ANCHOR: test_gdpr_export_token_single_use]
        record.write({"consumed": True})
        return record.user_id

    @api.model
    def consume_and_export(self, token):
        """The one RPC entrypoint the export daemon calls back into Odoo
        with. Validates+consumes the token, then materializes the full
        export payload (the in-memory dict plus every streamed generator
        fully drained into a list) as one JSON-serializable structure.

        Deliberate v1 simplification, disclosed here rather than silently
        assumed: this still ties up one Odoo worker for the time it takes to
        build this payload (a real, bounded, DB-bound cost), not zero time.
        What it does eliminate is the proposal's primary named failure mode --
        an Odoo worker blocked for the entire duration of a slow client's
        socket writes while tens of thousands of rows stream out. Draining
        the streamed generators here, in one RPC call, rather than exposing
        them as a separate offset/limit-batched RPC method the daemon could
        pull from repeatedly, is the one open architectural question this
        implementation did NOT resolve -- see GDPR_CSV_EXPORT.md's own
        updated Open Questions for why, and what re-architecting the
        generators to support real batched RPC pulls would take."""
        # # Verified by [@ANCHOR: test_gdpr_consume_and_export_payload]
        user = self._consume(token)
        # Reading a user's full export data (arbitrary res.users fields,
        # website.page/blog.post content, ...) needs broader read access
        # than gdpr_export_service_internal is granted -- deliberately, so
        # that narrow token-table account stays narrow (see _consume's own
        # docstring). zero_sudo.gdpr_service_internal is the existing,
        # already-scoped service account for exactly this: it already holds
        # read/write on res.users and read on website.page/blog.post/
        # blog.blog (see user_websites/security/ir.model.access.csv), the
        # same account _execute_gdpr_erasure() uses for erasure. Reusing it
        # here is the Service Account Pattern, not a second sudo().
        gdpr_svc_uid = self.env["zero_sudo.security.utils"]._get_service_uid(
            "zero_sudo.gdpr_service_internal"
        )
        user_as_gdpr_svc = user.with_user(gdpr_svc_uid)
        data = user_as_gdpr_svc._get_gdpr_export_data()
        streamed = user_as_gdpr_svc._get_gdpr_streamed_keys()
        materialized = {k: list(gen()) for k, gen in streamed.items()}
        # json.dumps/loads round-trip here is deliberate, not incidental: it
        # guarantees the payload crossing the JSON-2 RPC boundary is already
        # proven JSON-serializable (dates, Markup, etc. would otherwise fail
        # inside the RPC layer itself with a much less useful error), and it
        # keeps this method's contract identical regardless of how the RPC
        # transport itself happens to (de)serialize dicts.
        return json.loads(json.dumps({"data": data, "streamed": materialized}, default=str))
