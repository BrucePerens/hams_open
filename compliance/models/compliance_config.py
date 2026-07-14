# -*- coding: utf-8 -*-
# Copyright © Bruce Perens K6BP. Licensed under the GNU Affero General
# Public License v3.0 (AGPL-3.0).
from odoo import models, fields


class Website(models.Model):
    _inherit = "website"

    # AI Laziness Fix: Ensure cookies_bar is enabled by default for new websites.  # noqa: E501
    # This enforces compliance even for sites created after module
    # installation.
    cookies_bar = fields.Boolean(default=True)
