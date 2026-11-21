# Copyright © Bruce Perens K6BP.
# SPDX-License-Identifier: AGPL-3.0-or-later

# -*- coding: utf-8 -*-
"""Permanent, append-only record of every revoked API key (the key row itself is deleted to disable it)."""

from odoo import fields, models
from odoo.exceptions import UserError


class ApikeyRevocationLog(models.Model):
    _name = "apikey.revocation.log"
    _description = "API Key Revocation Log (append-only, permanent)"
    _order = "id desc"

    name = fields.Char(readonly=True, help="key name and id")
    owner_id = fields.Many2one("res.users", string="Key owner", ondelete="set null", readonly=True)
    owner_login = fields.Char(readonly=True)
    revoked_by_id = fields.Many2one("res.users", string="Revoked by", ondelete="set null", readonly=True)
    revoked_by_login = fields.Char(readonly=True)
    key_name = fields.Char(readonly=True)
    key_scope = fields.Char(readonly=True)
    key_id = fields.Integer(readonly=True)
    key_fingerprint = fields.Char(readonly=True, help="Prefix of SHA-256 of the stored key hash; never the secret.")
    source = fields.Selection(
        [("user", "Owner"), ("admin", "Administrator"), ("automatic", "Automatic")],
        readonly=True,
    )
    revoked_at = fields.Datetime(default=fields.Datetime.now, readonly=True)

    def write(self, vals):
        raise UserError(self.env._("The API key revocation log is append-only."))

    def unlink(self):
        raise UserError(self.env._("The API key revocation log is permanent and cannot be deleted."))
