# -*- coding: utf-8 -*-
# Copyright © Bruce Perens K6BP. AGPL-3.0.
import logging

from odoo import models, tools
from odoo.http import request

from odoo.addons.distributed_redis_cache.redis_pool import (
    get_redis_connection,
)
from odoo.addons.distributed_redis_cache.redis_cache import _local_cache, LRU_LOCK

_logger = logging.getLogger(__name__)

class IrHttp(models.AbstractModel):
    _inherit = "ir.http"

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
                try:
                    last_counter = cls._last_cache_counter
                except (KeyError, ValueError):
                    last_counter = None
                    
                if latest and latest != last_counter:
                    with LRU_LOCK:
                        _local_cache.clear()
                    cls._last_cache_counter = latest
            except (KeyError, ValueError) as e:   # Tested by [@ANCHOR: COMM_redis_cache_interceptor]
                _logger.exception("Failed to execute stateless Redis poll: %s", e)

        return super()._authenticate(endpoint)
