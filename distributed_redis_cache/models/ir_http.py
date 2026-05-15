# -*- coding: utf-8 -*-
# Copyright © Bruce Perens K6BP. AGPL-3.0.
import json
import logging
import threading
import concurrent.futures
import atexit

from odoo import models
from odoo.http import request

from odoo.addons.distributed_redis_cache.redis_pool import redis, redis_pool
from odoo.addons.distributed_redis_cache.redis_cache import invalidate_model_cache

_logger = logging.getLogger(__name__)

# Memory isolation: Thread-safe queue for the WSGI worker to consume instantly
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
        r_client = redis.Redis(connection_pool=redis_pool)
        pubsub = r_client.pubsub()
        pubsub.subscribe("odoo_cache_invalidation_bus")

        # Non-blocking listen to allow graceful thread management and connection resets
        while _listener_started:
            msg = pubsub.get_message(ignore_subscribe_messages=True, timeout=1.0)
            if msg and msg.get("type") == "message":
                data = msg.get("data")
                if data:
                    try:
                        payload = json.loads(data)
                        model_name = payload.get("model")
                        if model_name:
                            with _listener_lock:
                                _invalidation_queue.add(model_name)
                    except Exception as e:
                        logging.getLogger(__name__).warning("An error occurred: %s", e)
    except Exception as e:
        _logger.warning("Redis async listener thread disconnected: %s", e)
    finally:
        with _listener_lock:
            _listener_started = False


class IrHttp(models.AbstractModel):
    _inherit = "ir.http"

    @classmethod
    def _authenticate(cls, endpoint):
        # [@ANCHOR: redis_cache_interceptor]
        """
        Intercepts the request lifecycle to check for distributed cache invalidation signals.
        """
        global _listener_started

        # Ensure the background thread is spawned once per WSGI worker boot
        if not _listener_started:
            with _listener_lock:
                if not _listener_started:
                    _executor.submit(_redis_listener_thread)
                    _listener_started = True

        # O(1) Memory check entirely bypassing the network layer to prevent thread starvation
        if _invalidation_queue:
            with _listener_lock:
                models_to_clear = list(_invalidation_queue)
                _invalidation_queue.clear()

            for m in models_to_clear:
                # Security: Validate model name before accessing env
                if m and isinstance(m, str) and m in request.env:
                    invalidate_model_cache(request.env, m)
                    _logger.info("Distributed cache cleared for model: %s", m)

        return super()._authenticate(endpoint)
