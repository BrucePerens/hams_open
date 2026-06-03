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
                # Inject a cryptographically secure, extremely large password
                # to guarantee the account cannot be logged into interactively.
                vals["password"] = secrets.token_urlsafe(128)
        return super().create(vals_list)
