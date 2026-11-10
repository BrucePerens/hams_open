# -*- coding: utf-8 -*-
# Copyright © Bruce Perens K6BP.
# SPDX-License-Identifier: AGPL-3.0-or-later
from odoo import fields, models


class SecurityLog(models.Model):
    _inherit = "zero_sudo.security.log"

    reason = fields.Selection(
        selection_add=[("secret_canary", "Secret Canary Or Registered Secret Seen In Output")],
        ondelete={"secret_canary": "cascade"},
    )
