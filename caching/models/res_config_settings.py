# -*- coding: utf-8 -*-
# Copyright © HAMS project. AGPL-3.0.
from odoo import api, fields, models


class ResConfigSettings(models.TransientModel):
    _inherit = "res.config.settings"

    caching_safe_quota_mb = fields.Integer(
        string="Safe Quota (MB)",
        help=(
            "Maximum total size in MB of cached files. If total "
            "files exceed this, the max single file size cached "
            "will be lowered dynamically."
        ),
    )

    caching_invalidation_version = fields.Integer(
        string="Cache Invalidation Version",
        readonly=True,
        help=(
            "Increment this value to force users' browsers to "
            "immediately wipe their cache."
        ),
    )

    @api.model
    def get_values(self):
        res = super(ResConfigSettings, self).get_values()
        
        website_id = self.env.context.get('website_id')
        if not website_id:
            try:
                website_id = self.env['website'].get_current_website().id
            except Exception:
                pass
                
        caching_safe_quota_mb = 35
        caching_inversion = 1
        
        if website_id:
            website = self.env['website'].browse(website_id)
            caching_safe_quota_mb = website.caching_safe_quota_mb
            caching_inversion = website.caching_invalidation_version

        res.update(
            caching_safe_quota_mb=caching_safe_quota_mb,
            caching_invalidation_version=caching_inversion,
        )
        return res

    def set_values(self):
        super(ResConfigSettings, self).set_values()
        if self.website_id:
            self.website_id.caching_safe_quota_mb = self.caching_safe_quota_mb

    def action_force_cache_invalidation(self):
        """Increments the cache version for the current website."""
        self.ensure_one()
        if self.website_id:
            self.website_id.caching_invalidation_version += 1
        return {
            "type": "ir.actions.client",
            "tag": "reload",
        }
