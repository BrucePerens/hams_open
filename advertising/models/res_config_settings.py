# -*- coding: utf-8 -*-
# Copyright © HAMS project. AGPL-3.0-or-later.
from odoo import fields, models


class ResConfigSettings(models.TransientModel):
    _inherit = "res.config.settings"

    google_adsense_client_id = fields.Char(
        related="website_id.google_adsense_client_id", readonly=False
    )
    google_adsense_footer_slot_id = fields.Char(
        related="website_id.google_adsense_footer_slot_id", readonly=False
    )
    google_adsense_sidebar_slot_id = fields.Char(
        related="website_id.google_adsense_sidebar_slot_id", readonly=False
    )
