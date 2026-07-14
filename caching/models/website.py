# -*- coding: utf-8 -*-
# Copyright © HAMS project. AGPL-3.0.
from odoo import fields, models


class Website(models.Model):
    _inherit = "website"
    # This model is multi-tenant and multi-website.
    # Each website can have its own caching configuration (quota and version).

    caching_safe_quota_mb = fields.Integer(
        string="Safe Quota (MB)",
        default=35,
        help=(
            "Maximum total size in MB of cached files. If total "
            "files exceed this, the max single file size cached "
            "will be lowered dynamically."
        ),
    )

    caching_pwa_theme_color = fields.Char(string="PWA Theme Color", default="#875A7B")
    caching_pwa_background_color = fields.Char(string="PWA Background Color", default="#ffffff")

    caching_invalidation_version = fields.Integer(
        string="Cache Invalidation Version",
        default=1,
        help=(
            "Increment this value to force users' browsers to "
            "immediately wipe their cache."
        ),
    )
