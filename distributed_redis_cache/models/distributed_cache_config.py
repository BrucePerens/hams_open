# Copyright © Bruce Perens K6BP.
# SPDX-License-Identifier: AGPL-3.0-or-later

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
        # [@ANCHOR: COMM_manual_cache_invalidation]
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
        # [@ANCHOR: COMM_check_redis_status_logic]
        use_redis = bool(redis and redis_pool)


        # Removed test_mode bypass to satisfy test fidelity mandates.

        fallback_msg = """Redis connection is not configured or unavailable. Local fallback cache is active."""
        status_msg = _(fallback_msg)
        msg_type = "warning"

        if use_redis:
            try:
                r = get_redis_connection(self.env)
                r.ping()
                status_msg = _("Redis connection is healthy.")
                msg_type = "success"
            except redis.RedisError as e:
                _logger.warning("Redis connection check failed: %s", e)
                status_msg = _("Redis connection failed: %s") % e
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
