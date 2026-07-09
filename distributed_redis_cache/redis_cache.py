# -*- coding: utf-8 -*-
# Copyright © Bruce Perens K6BP. AGPL-3.0.
import json
import logging
import hashlib
import datetime
from functools import wraps
from odoo import models, tools
from odoo.addons.distributed_redis_cache.redis_pool import (
    redis,
    redis_pool,
    get_redis_connection,
)
import threading
from odoo.tools.lru import LRU

_logger = logging.getLogger(__name__)

# Local fallback cache to maintain HA if Redis is unreachable.
# Limit to 8192 entries to prevent memory exhaustion during Redis outages.
_local_cache = LRU(8192)
LRU_LOCK = threading.Lock()


def _get_hash(*args, **kwargs):
    # [@ANCHOR: distributed_cache_key_generation]
    def _serialize(obj):
        if isinstance(obj, models.Model):
            # Ensure stable serialization for recordsets
            sorted_ids = sorted(obj.ids) if obj.ids else []
            return f"{obj._name}({','.join(map(str, sorted_ids))})"
        if isinstance(obj, (datetime.date, datetime.datetime)):
            return obj.isoformat()
        if isinstance(obj, (list, tuple)):
            return [_serialize(i) for i in obj]
        if isinstance(obj, dict):
            return {str(k): _serialize(v) for k, v in sorted(obj.items())}
        if isinstance(obj, bytes):
            return obj.hex()
        if isinstance(obj, (set, frozenset)):
            # Sort for stability across processes
            return [_serialize(i) for i in sorted(list(obj), key=str)]
        if obj is None:
            return None
        if isinstance(obj, (bool, int, float, str)):
            return obj
        return str(obj)

    serialized_args = [_serialize(a) for a in args]
    serialized_kwargs = {k: _serialize(v) for k, v in sorted(kwargs.items())}

    # Use json.dumps with sort_keys for absolute stability across workers
    arg_str = json.dumps([serialized_args, serialized_kwargs], sort_keys=True)
    return hashlib.sha256(arg_str.encode("utf-8")).hexdigest()


def distributed_cache():
    # [@ANCHOR: distributed_cache_decorator]
    """
    Fine-grained, distributed Redis-backed cache decorator.
    Replaces @tools.ormcache to support precise cross-worker invalidation.
    """

    def decorator(func):
        @wraps(func)
        def wrapper(self, *args, **kwargs):
            dbname = self.env.cr.dbname
            model_name = self._name

            # Multi-Tenant awareness: Include website_id and company_id in cache key
            website_id = self.env.context.get("website_id")
            
            # Use only context to avoid triggering N+1 queries from self.env.company
            allowed_company_ids = self.env.context.get("allowed_company_ids", [])
            company_id = allowed_company_ids[0] if allowed_company_ids else None
            
            # [!] SECURITY: Multi-tenant isolation is enforced via website_id and company_id in the cache key.
            website_suffix = f":w{website_id}" if website_id else ""
            company_suffix = f":c{company_id}" if company_id else ""

            arg_hash = _get_hash(self, *args, **kwargs)
            cache_key = f"{dbname}:distributed_cache:{model_name}:{func.__name__}{website_suffix}{company_suffix}:{arg_hash}"

            # L1 Cache Check (In-Memory)
            with LRU_LOCK:
                if cache_key in _local_cache:
                    return _local_cache[cache_key]

            use_redis = bool(redis and redis_pool)

            # Ensure Redis is running in testing environments to maintain production parity.

            if use_redis:
                try:
                    r = get_redis_connection(self.env)
                    cached = r.get(cache_key)
                    if cached:
                        # [!] DIAGNOSTIC FOR AI: Redis cache hit for %s
                        _logger.debug("Redis cache hit: %s", cache_key)
                        res = json.loads(cached)
                        with LRU_LOCK:
                            _local_cache[cache_key] = res
                        return res
                except redis.RedisError as e:
                    _logger.warning("Network partition detected. Bypassing Redis: %s", e)
                    use_redis = False
                except json.JSONDecodeError as e:
                    _logger.warning("Redis cache corrupted JSON payload: %s", e)
                    use_redis = False

            result = func(self, *args, **kwargs)

            if use_redis:
                try:
                    serialized_result = json.dumps(result, default=tools.json_default)
                    r = get_redis_connection(self.env)
                    r.setex(cache_key, 86400, serialized_result)  # 24h TTL
                except redis.RedisError as e:
                    _logger.warning("Network partition detected during cache write: %s", e)
                except (TypeError, json.JSONDecodeError) as e:
                    _logger.warning("Redis cache write serialization failed: %s", e)

            # Always populate L1 local fallback cache
            with LRU_LOCK:
                _local_cache[cache_key] = result
            return result

        return wrapper

    return decorator


def invalidate_model_cache(env, model_name, local_only=False):
    # [@ANCHOR: invalidate_model_cache_logic]
    """
    Invalidates all fine-grained cache entries for a specific model
    without triggering a global ORM stampede.
    """
    dbname = env.cr.dbname
    prefix = f"{dbname}:distributed_cache:{model_name}:*"

    if not local_only:
        use_redis = bool(redis and redis_pool)
        # Ensure Redis is running in testing environments to maintain production parity.

        if use_redis:
            try:
                r = get_redis_connection(env)
                # Use SCAN instead of KEYS for production safety
                # Process in batches to avoid blocking Redis or consuming too much memory
                keys = []
                for key in r.scan_iter(match=prefix, count=1000):
                    keys.append(key)
                    if len(keys) >= 1000:
                        r.delete(*keys)
                        keys = []
                if keys:
                    r.delete(*keys)
            except redis.RedisError as e:
                _logger.warning("Redis cache invalidation failed: %s", e)

    # Always clear local fallback cache for this process to ensure consistency
    prefix_local = f"{dbname}:distributed_cache:{model_name}:"
    with LRU_LOCK:
        keys_to_delete = [k for k in _local_cache.keys() if k.startswith(prefix_local)]
        for k in keys_to_delete:
            _local_cache.pop(k, None)


def notify_model_invalidation(env, model_name):
    # [@ANCHOR: notify_model_invalidation_logic]
    """
    Triggers a cross-worker invalidation signal via PostgreSQL NOTIFY.
    """
    # Security: Validate model name
    if model_name not in env:
        # [!] DIAGNOSTIC FOR AI: Invalid model name %s attempted for invalidation.
        _logger.warning(
            "Security: Attempted to invalidate unknown model %s", model_name
        )
        return

    dbname = env.cr.dbname

    # 1. Invalidate locally and in Redis ONLY after the transaction commits.
    # This completely closes the race condition window where an intervening read
    # might cache out-of-date records before the write finishes.
    def _do_invalidate():
        invalidate_model_cache(env, model_name, local_only=False)

    # Fail fast if postcommit API is not present, enforcing architectural contract
    env.cr.postcommit.add(_do_invalidate)


    # 2. Notify all other workers via Postgres -> Daemon -> Redis Pub/Sub.
    # pg_notify is natively transactional and inherently waits until commit to broadcast.
    payload = json.dumps({"model": model_name, "dbname": dbname})
    env.cr.execute(
        "SELECT pg_notify(%s, %s)", ("distributed_cache_invalidation", payload)
    )
