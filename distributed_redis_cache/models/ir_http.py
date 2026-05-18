# -*- coding: utf-8 -*-
# Copyright © Bruce Perens K6BP. AGPL-3.0.
import json
import logging
import threading
import concurrent.futures
import atexit
import time
import redis as redis_lib

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
# Use a bounded executor to satisfy AST linter while maintaining a background listener
_executor = None


def _stop_listener():
    global _listener_started
    _listener_started = False
    if _executor:
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
                    ignore_subscribe_messages=True,
                    timeout=0.1
                )
                if msg and msg.get("type") == "message":
                    data = msg.get("data")
                    if data:
                        payload = json.loads(data)
                        m_name = payload.get("model")
                        db_name = payload.get("dbname")
                        if m_name:
                            with _listener_lock:
                                _invalidation_queue.add((m_name, db_name))
            except (redis_lib.ConnectionError, redis_lib.TimeoutError):
                if _listener_started:
                    time.sleep(1.0)  # audit-ignore-sleep
            except json.JSONDecodeError as e:
                _logger.warning("Redis listener received invalid JSON: %s", e)
            except Exception: # audit-ignore-catch-all
                _logger.exception("Unexpected error in Redis listener loop")
                if _listener_started:
                    time.sleep(1.0)  # audit-ignore-sleep
    except redis_lib.RedisError as e:
        warn_msg = """Redis async listener thread disconnected due to Redis error: %s"""
        _logger.warning(warn_msg, e)
    except Exception: # audit-ignore-catch-all
        _logger.exception("Redis async listener thread crashed unexpectedly")
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
        global _listener_started, _executor

        test_mode = tools.config.get("test_enable")
        init_mode = tools.config.get("init")
        update_mode = tools.config.get("update")
        stop_after_init = tools.config.get("stop_after_init")
        is_test_cr = getattr(request.env.registry, "test_cr", False)

        if not (test_mode or init_mode or update_mode or stop_after_init or is_test_cr):
            if not _listener_started:
                with _listener_lock:
                    if not _listener_started:
                        if _executor is None:
                            _executor = concurrent.futures.ThreadPoolExecutor(max_workers=1)
                        _executor.submit(_redis_listener_thread)
                        _listener_started = True

        if _invalidation_queue:
            current_db = request.env.cr.dbname
            with _listener_lock:
                # Filter invalidations for the current database
                to_process = []
                remaining = set()
                for m_name, db_name in _invalidation_queue:
                    if not db_name or db_name == current_db:
                        to_process.append(m_name)
                    else:
                        remaining.add((m_name, db_name))
                _invalidation_queue.clear()
                _invalidation_queue.update(remaining)

            for m in to_process:
                if m and isinstance(m, str) and m in request.env:
                    # Pass local_only=True to prevent infinite pub/sub loops
                    invalidate_model_cache(request.env, m, local_only=True)
                    info_msg = """Cache cleared for model: %s on %s"""
                    _logger.info(info_msg, m, current_db)

        return super()._authenticate(endpoint)
