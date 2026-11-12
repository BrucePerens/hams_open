# -*- coding: utf-8 -*-
# Copyright © Bruce Perens K6BP.
# SPDX-License-Identifier: AGPL-3.0-or-later
from odoo import models, fields


# [@ANCHOR: compliance_compliance_config_website]
class Website(models.Model):
    _inherit = "website"

    # AI Laziness Fix: Ensure cookies_bar is enabled by default for new websites.
    # This enforces compliance even for sites created after module
    # installation.
    cookies_bar = fields.Boolean(default=True)
