# -*- coding: utf-8 -*-
# Copyright © Bruce Perens K6BP. AGPL-3.0.
import json
import logging
import threading
import concurrent.futures
import atexit
import time

from odoo import models, tools
from odoo.http import request

from odoo.addons.distributed_redis_cache.redis_pool import (
    redis,
    redis_pool,
)
from odoo.addons.distributed_redis_cache.redis_cache import (
    invalidate_model_cache,
)


_logger = logging.getLogger(__name__)

_invalidation_queue = set()
_listener_started = False
_listener_lock = threading.Lock()
_executor = concurrent.futures.ThreadPoolExecutor(max_workers=1)


def _stop_listener():
    global _listener_started
    _listener_started = False
    _executor.shutdown(wait=False)


atexit.register(_stop_listener)


def _redis_listener_thread():
    global _listener_started
    if not redis or not redis_pool:
        return
    try:
        r_client = redis.Redis(
            connection_pool=redis_pool, socket_timeout=2.0
        )
        pubsub = r_client.pubsub()
        pubsub.subscribe("odoo_cache_invalidation_bus")

        while _listener_started:
            try:
                msg = pubsub.get_message(
                    ignore_subscribe_messages=True
                )
                if msg and msg.get("type") == "message":
                    data = msg.get("data")
                    if data:
                        payload = json.loads(data)
                        m_name = payload.get("model")
                        if m_name:
                            with _listener_lock:
                                _invalidation_queue.add(m_name)
                time.sleep(0.5)  # audit-ignore-sleep
            except (redis.ConnectionError, redis.TimeoutError):
                time.sleep(1.0)  # audit-ignore-sleep
            except Exception as e:
                logging.getLogger(__name__).warning("Error: %s", e)
                time.sleep(1.0)  # audit-ignore-sleep
    except Exception as e:
        warn_msg = """Redis async listener thread disconnected: %s"""
        _logger.warning(warn_msg, e)
    finally:
        with _listener_lock:
            _listener_started = False


class IrHttp(models.AbstractModel):
    _inherit = "ir.http"

    @classmethod
    def _authenticate(cls, endpoint):
        # [@ANCHOR: redis_cache_interceptor]
        """
        Intercepts request lifecycle to check cache invalidation.
        """
        global _listener_started

        test_mode = tools.config.get("test_enable")
        is_test_cr = getattr(request.env.registry, "test_cr", False)

        if not (test_mode or is_test_cr):
            if not _listener_started:
                with _listener_lock:
                    if not _listener_started:
                        _executor.submit(_redis_listener_thread)
                        _listener_started = True

        if _invalidation_queue:
            with _listener_lock:
                models_to_clear = list(_invalidation_queue)
                _invalidation_queue.clear()

            for m in models_to_clear:
                if m and isinstance(m, str) and m in request.env:
                    invalidate_model_cache(request.env, m)
                    info_msg = """Cache cleared for model: %s"""
                    _logger.info(info_msg, m)

        return super()._authenticate(endpoint)
