# -*- coding: utf-8 -*-
from odoo import models, fields


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
    )
