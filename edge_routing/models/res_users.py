# -*- coding: utf-8 -*-
# Part of Odoo. See LICENSE file for full copyright and licensing details.
#
# This file is part of hams_open, an open source module.
# License: AGPL-3.0

from odoo import models, api, fields

import logging

_logger = logging.getLogger(__name__)

from odoo.addons.distributed_redis_cache.redis_cache import distributed_cache
class ResUsersEdgeRouting(models.Model):  # burn-ignore-env
    """
    Extends res.users with edge.routing.mixin to provide high-performance
    vanity URL routing and slug caching.
    """

    _name = "res.users"
    _inherit = ["res.users", "edge.routing.mixin"]

    @api.model
    @distributed_cache()
    def get_record_by_slug(self, slug, override_svc_uid=None):
        res = super().get_record_by_slug(slug, override_svc_uid=override_svc_uid)
        if not res and slug:
            # Virtual Slug Fallback: Check if the URL matches their unique login (e.g. Callsign)
            if override_svc_uid:
                target_env = self.with_user(override_svc_uid).env
            else:
                if self.env.registry.loaded:
                    self.env.cr.execute("SELECT 1 FROM ir_model_data WHERE module=%s AND name=%s", ('edge_routing', 'edge_routing_service_account')) # audit-ignore-sql
                    if self.env.cr.fetchone():
                        try:
                            target_env = self.env["zero_sudo.security.utils"]._get_service_env(
                                "edge_routing.edge_routing_service_account"
                            )
                        except Exception as e:  # audit-ignore-catch-all
                            _logger.warning("Failed to access website settings: %s", e)
                            target_env = self.env
                    else:
                        target_env = self.env
                else:
                    target_env = self.env

            user = (
                target_env["res.users"]
                .with_context(active_test=False)
                .search([("login", "=ilike", str(slug).lower())], limit=1)
            )
            return user.id if user else False
        return res
