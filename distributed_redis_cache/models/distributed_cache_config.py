# This software is distributed under the terms of the Affero General Public License (AGPL-3).

# -*- coding: utf-8 -*-
import logging
from odoo import models, fields, _
from odoo.addons.distributed_redis_cache.redis_cache import (
    notify_model_invalidation,
)
from odoo.addons.distributed_redis_cache.redis_pool import (
    redis,
    redis_pool,
    get_redis_connection,
)

_logger = logging.getLogger(__name__)


class DistributedCacheConfig(models.TransientModel):
    _name = "distributed.cache.config"
    _description = "Distributed Cache Configuration"
    name = fields.Char(string="Name", default=lambda self: self._description)

    model_id = fields.Many2one(
        "ir.model",
        string="Model to Invalidate",
        help="Select a model to flush its specific cache.",
    )

    def action_invalidate_model_cache(self):
        # [@ANCHOR: manual_cache_invalidation]
        self.ensure_one()
        if self.model_id:
            model_name = self.model_id.model
            notify_model_invalidation(self.env, model_name)
            return {
                "type": "ir.actions.client",
                "tag": "display_notification",
                "params": {
                    "title": _("Success"),
                    "message": _("Cache invalidated for model %s") % self.model_id.model,
                    "type": "success",
                    "sticky": False,
                },
            }

    def check_redis_status(self):
        # [@ANCHOR: check_redis_status_logic]
        use_redis = bool(redis and redis_pool)

        # Use zero_sudo security utils for system parameter read to comply with security mandates
        integration_active = self.env["zero_sudo.security.utils"]._get_system_param(
            "distributed_redis_cache.test_integration_active"
        )
        # Removed test_mode bypass to satisfy test fidelity mandates.

        status_msg = _(
            "Redis connection is not configured or unavailable. Local fallback cache is active."
        )
        msg_type = "warning"

        if use_redis:
            try:
                r = get_redis_connection(self.env)
                r.ping()
                status_msg = _("Redis connection is healthy.")
                msg_type = "success"
            except redis.RedisError as e:
                _logger.warning("Redis connection check failed: %s", e)
                status_msg = _("Redis connection failed: %s", e)
                msg_type = "danger"

        return {
            "type": "ir.actions.client",
            "tag": "display_notification",
            "params": {
                "title": _("Redis Status"),
                "message": status_msg,
                "type": msg_type,
                "sticky": False,
            },
        }
