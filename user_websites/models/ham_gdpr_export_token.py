# SPDX-License-Identifier: AGPL-3.0-or-later
# -*- coding: utf-8 -*-
# Copyright © Bruce Perens K6BP. Licensed under the GNU Affero General Public License v3.0 or later (AGPL-3.0-or-later).

import json
import secrets
from datetime import timedelta

from odoo import models, fields, api
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

    _token_unique = models.Constraint(
        "UNIQUE(token)", "Export tokens must be unique."
    )

    @api.model
    def create_for_current_user(self):
        """Called from the authenticated /my/privacy/export.zip controller,
        as the requesting user's own session -- this is the only path that
        creates a token, so a token's user_id is always the identity Odoo
        itself already authenticated, never a caller-supplied value."""
        token = secrets.token_urlsafe(32)
        self.sudo().create({"user_id": self.env.uid, "token": token})
        return token

    @api.model
    def _consume(self, token):
        """Validates and atomically consumes a token, returning the res.users
        record it was issued for. Raises AccessError on any failure (missing,
        already consumed, expired) -- refuses to distinguish *why* to a caller
        that doesn't already hold a valid token, the same fail-closed posture
        as every other credential check in this codebase.

        Only ever called by the gdpr_export_service_internal daemon account
        (see security/gdpr_export_security.xml) -- deliberately sudo()'d
        internally so that account doesn't need broad res.users read access
        of its own; this method is the one, narrow, disclosed elevated-
        privilege surface, not a general grant. Every other GDPR export data
        access this feature makes still goes through the existing
        _get_gdpr_export_data()/_get_gdpr_streamed_keys() contract, unchanged."""
        record = self.sudo().search(
            [("token", "=", token), ("consumed", "=", False)], limit=1
        )
        if not record:
            raise AccessError("Invalid or already-used export token.")
        cutoff = fields.Datetime.now() - timedelta(minutes=TOKEN_EXPIRY_MINUTES)
        if record.create_date < cutoff:
            raise AccessError("Export token has expired.")
        # Consume first, fetch after: a failure while building the export
        # payload below must not leave a still-valid, replayable token behind.
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
        user = self._consume(token)
        data = user.sudo()._get_gdpr_export_data()
        streamed = user.sudo()._get_gdpr_streamed_keys()
        materialized = {k: list(gen()) for k, gen in streamed.items()}
        # json.dumps/loads round-trip here is deliberate, not incidental: it
        # guarantees the payload crossing the JSON-2 RPC boundary is already
        # proven JSON-serializable (dates, Markup, etc. would otherwise fail
        # inside the RPC layer itself with a much less useful error), and it
        # keeps this method's contract identical regardless of how the RPC
        # transport itself happens to (de)serialize dicts.
        return json.loads(json.dumps({"data": data, "streamed": materialized}, default=str))
