# -*- coding: utf-8 -*-
import secrets

from odoo import api, fields, models


class ResUsersZeroSudo(models.Model):
    _inherit = "res.users"

    is_service_account = fields.Boolean(
        # [@ANCHOR: is_service_account_field]
        # Verified by [@ANCHOR: test_is_service_account_field]
        # Tests [@ANCHOR: story_login_blocking]
        # Tests [@ANCHOR: journey_service_account_lifecycle]
        string="Is Service Account",
        default=False,
        help="Flags this user as an internal service account. Prevents interactive web logins.",
        groups="base.group_system",
    )

    @api.model_create_multi
    def create(self, vals_list):
        # [@ANCHOR: service_account_password_generation]
        # Verified by [@ANCHOR: test_service_account_password]
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
                self.env.cr.execute(
                    "SELECT id FROM res_users WHERE id IN %s AND is_service_account = True",
                    (tuple(self.ids),)
                )
                service_accounts_ids = [r[0] for r in self.env.cr.fetchall()]
                if service_accounts_ids:
                    regular_accounts = self.filtered(lambda r: r.id not in service_accounts_ids)
                    service_accounts = self.filtered(lambda r: r.id in service_accounts_ids)
                    
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
