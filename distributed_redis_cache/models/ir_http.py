# -*- coding: utf-8 -*-
# SPDX-License-Identifier: AGPL-3.0-or-later
import logging

from odoo import models, tools
from odoo.http import request

from odoo.addons.distributed_redis_cache.redis_pool import (
    get_redis_connection,
    redis,
)
from odoo.addons.distributed_redis_cache.redis_cache import _local_cache, LRU_LOCK

_logger = logging.getLogger(__name__)

class IrHttp(models.AbstractModel):
    _inherit = "ir.http"

    # Explicit class-level default so cls._last_cache_counter is never
    # missing (avoiding both a banned catch-all AttributeError and a
    # banned 3-argument getattr() to probe for it) -- it's genuinely
    # unset until the first successful Redis poll below writes a real
    # counter value onto the class.
    _last_cache_counter = None

    @classmethod
    def _authenticate(cls, endpoint):
        # [@ANCHOR: COMM_redis_cache_interceptor]
        """
        Intercepts request lifecycle to check cache invalidation.
        """
        init_mode = tools.config.get("init")
        update_mode = tools.config.get("update")
        stop_after_init = tools.config.get("stop_after_init")

        if not (init_mode or update_mode or stop_after_init):
            try:
                r = get_redis_connection(request.env)
                latest = r.get("global_cache_invalidation_counter")
                last_counter = cls._last_cache_counter

                if latest and latest != last_counter:
                    with LRU_LOCK:
                        _local_cache.clear()
                    cls._last_cache_counter = latest
            except redis.RedisError as e:   # Verified by [@ANCHOR: COMM_redis_cache_interceptor]
                _logger.warning("Failed to execute stateless Redis poll: %s", e)

        return super()._authenticate(endpoint)
