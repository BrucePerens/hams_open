# -*- coding: utf-8 -*-
# Part of Odoo. See LICENSE file for full copyright and licensing details.
#
# This file is part of hams_open, an open source module.
# SPDX-License-Identifier: AGPL-3.0-or-later

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
        # [@ANCHOR: zero_sudo:COMM_is_service_account_field]
        # ---
        # # Verified by [@ANCHOR: zero_sudo:COMM_test_is_service_account_field]
        # ---
        # Tests [@ANCHOR: zero_sudo:COMM_story_login_blocking]
        # ---
        # Tests [@ANCHOR: zero_sudo:COMM_journey_service_account_lifecycle]
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
        # [@ANCHOR: zero_sudo:COMM_service_account_password_generation]
        # ---
        # # Verified by [@ANCHOR: zero_sudo:COMM_COMM_test_service_account_password]
        for vals in vals_list:
            if vals.get("is_service_account"):
                # Ensure no password for service accounts
                vals.pop("password", None)
                vals["password"] = secrets.token_hex(32)
        return super().create(vals_list)

    # [@ANCHOR: zero_sudo:res_users_write]
    def write(self, vals):
        # Bug-hunt fix: previously only forced a random password when
        # is_service_account was being set to a TRUTHY value (becoming a
        # service account). De-designating one (is_service_account: False)
        # while ALSO supplying a real password in the SAME call skipped
        # this branch (its value isn't truthy) and skipped the elif branch
        # below too (it requires "is_service_account" not in vals at all)
        # -- falling through to a plain super().write(vals) that applied
        # the caller-chosen password as-is, silently restoring a loginable
        # credential on a record that had been a service account moments
        # earlier. Any transition of this field, in either direction, now
        # forces a fresh random password in the same write, matching
        # create()'s own existing behavior. Setting a REAL password for a
        # newly-human account is a deliberate follow-up write, not
        # something this call should ever silently allow to ride along.
        if "is_service_account" in vals:
            vals = dict(vals)
            vals["password"] = secrets.token_hex(32)
        elif "password" in vals:
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
