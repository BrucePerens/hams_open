# -*- coding: utf-8 -*-
from odoo import fields, models


class ResConfigSettings(models.TransientModel):
    _inherit = "res.config.settings"

    caching_safe_quota_mb = fields.Integer(
        string="Safe Quota (MB)",
        related="website_id.caching_safe_quota_mb",
        readonly=False,
        help=(
            "Maximum total size in MB of cached files. If total "
            "files exceed this, the max single file size cached "
            "will be lowered dynamically."
        ),
    )

    caching_invalidation_version = fields.Integer(
        string="Cache Invalidation Version",
        related="website_id.caching_invalidation_version",
        readonly=True,
        help=(
            "Increment this value to force users' browsers to "
            "immediately wipe their cache."
        ),
    )

    def action_force_cache_invalidation(self):
        """Increments the cache version for the current website."""
        self.ensure_one()
        if not self.website_id:
            return False
        # We bypass the related field to ensure we're writing
        # to the correct website record.
        self.website_id.caching_invalidation_version += 1
        return {
            "type": "ir.actions.client",
            "tag": "reload",
        }
