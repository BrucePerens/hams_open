# -*- coding: utf-8 -*-
# Copyright © Bruce Perens K6BP. AGPL-3.0.
import json
import logging
import threading
import atexit
from concurrent.futures import ThreadPoolExecutor
import time
import sys

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
# Use a bounded ThreadPoolExecutor as per architectural mandates to prevent DOS.
BACKGROUND_EXECUTOR = ThreadPoolExecutor(max_workers=1, thread_name_prefix="distributed_cache_listener")


def _redis_listener_thread_loop():
    global _listener_started
    _logger.info("Starting Redis Distributed Cache Listener Thread...")
    if not redis or not redis_pool:
        _listener_started = False
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
        _logger.info("Redis Distributed Cache Listener Thread stopped.")


def _stop_listener():
    global _listener_started
    _listener_started = False
    BACKGROUND_EXECUTOR.shutdown(wait=False, cancel_futures=True)


atexit.register(_stop_listener)


class IrHttp(models.AbstractModel):
    _inherit = "ir.http"

    @classmethod
    def _authenticate(cls, endpoint):
        # [@ANCHOR: redis_cache_interceptor]
        """
        Intercepts request lifecycle to check cache invalidation.
        """
        global _listener_started

        # Ensure the background thread is spawned once per WSGI worker boot.
        # CRITICAL: Skip during database initialization, tests, or shutdown modes to avoid hanging processes.
        # This allows tools/test_runner.py to rebuild the database without being blocked by background threads.
        if not _listener_started and not tools.config.get("test_enable") and not tools.config.get("init") and not tools.config.get("stop_after_init"):
            # Only start if we are in a real request context with a database,
            # which usually doesn't happen during pure DB initialization/module loading.
            if request and getattr(request, 'db', False):
                with _listener_lock:
                    if not _listener_started:
                        _listener_started = True
                        BACKGROUND_EXECUTOR.submit(_redis_listener_thread_loop)

        if _invalidation_queue:
            with _listener_lock:
                models_to_clear = list(_invalidation_queue)
                _invalidation_queue.clear()

            for m in models_to_clear:
                if m and isinstance(m, str) and m in request.env:
                    invalidate_model_cache(request.env, m, local_only=True)
                    _logger.info("Distributed cache cleared for model: %s", m)

        return super()._authenticate(endpoint)
