# -*- coding: utf-8 -*-
# Part of Odoo. See LICENSE file for full copyright and licensing details.
#
# This file is part of hams_open, an open source module.
# License: AGPL-3.0

import secrets

from odoo import api, fields, models


class ResUsersZeroSudo(models.Model):
    _inherit = "res.users"

    security_log_ids = fields.One2many(
        "zero_sudo.security.log",
        "user_id",
        string="Security Logs",
    )

    is_service_account = fields.Boolean(
        # [@ANCHOR: COMM_is_service_account_field]
        # ---
        # Verified by [@ANCHOR: COMM_test_is_service_account_field]
        # ---
        # Tests [@ANCHOR: COMM_story_login_blocking]
        # ---
        # Tests [@ANCHOR: COMM_journey_service_account_lifecycle]
        string="Is Service Account",
        default=False,
        help=(
            "Flags this user as an internal service account. "
            "Prevents interactive web logins."
        ),
        groups="base.group_system",
    )

    @api.model_create_multi
    def create(self, vals_list):
        # [@ANCHOR: COMM_service_account_password_generation]
        # ---
        # Verified by [@ANCHOR: COMM_COMM_test_service_account_password]
        for vals in vals_list:
            if vals.get("is_service_account"):
                # Ensure no password for service accounts
                vals.pop("password", None)
                vals["password"] = secrets.token_hex(32)
        return super().create(vals_list)

    def write(self, vals):
        if vals.get("is_service_account"):
            vals["password"] = secrets.token_hex(32)
        elif "password" in vals and "is_service_account" not in vals:
            if self.ids:
                service_accounts = self.filtered("is_service_account")
                if service_accounts:
                    regular_accounts = self - service_accounts
                    
                    res = True
                    if regular_accounts:
                        res = super(ResUsersZeroSudo, regular_accounts).write(vals)
                    
                    vals_no_pw = vals.copy()
                    vals_no_pw.pop("password", None)
                    if vals_no_pw:
                        res2 = super(ResUsersZeroSudo, service_accounts).write(vals_no_pw)
                        res = res and res2
                    return res
        return super().write(vals)
