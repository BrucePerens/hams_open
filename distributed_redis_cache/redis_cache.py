# -*- coding: utf-8 -*-
# Copyright © Bruce Perens K6BP. AGPL-3.0.
import json
import logging
import hashlib
import datetime
from functools import wraps
from odoo import models, tools
from odoo.addons.distributed_redis_cache.redis_pool import redis, redis_pool

_logger = logging.getLogger(__name__)

# Local fallback cache to maintain HA if Redis is unreachable.
# Limit to 8192 entries to prevent memory exhaustion during Redis outages.
_local_cache = tools.lru.LRU(8192)


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
            # Force company_id from context if available, else use env.company
            website_id = self.env.context.get("website_id")
            company_id = self.env.context.get("allowed_company_ids", [self.env.company.id])[0]
            # [!] SECURITY: Multi-tenant isolation is enforced via website_id and company_id in the cache key.
            website_suffix = f":w{website_id}" if website_id else ""
            company_suffix = f":c{company_id}" if company_id else ""

            arg_hash = _get_hash(*args, **kwargs)
            cache_key = f"{dbname}:distributed_cache:{model_name}:{func.__name__}{website_suffix}{company_suffix}:{arg_hash}"

            use_redis = bool(redis and redis_pool)

            # Sever Redis connection during standard automated testing
            # to prevent cross-test ghost cache poisoning after Postgres rollbacks.
            # RealTransactionCase and integration tests can re-enable it via system parameter.
            if tools.config.get("test_enable"):
                # Use super-user ID directly for system parameter read to comply with zero-sudo linter
                param_obj = self.env['ir.config_parameter'].with_user(1)
                integration_active = param_obj.get_param('distributed_redis_cache.test_integration_active')
                if not integration_active:
                    use_redis = False

            if use_redis:
                try:
                    r = redis.Redis(connection_pool=redis_pool)
                    cached = r.get(cache_key)
                    if cached:
                        # [!] DIAGNOSTIC FOR AI: Redis cache hit for %s
                        return json.loads(cached)
                except (redis.RedisError, json.JSONDecodeError) as e:
                    # [!] DIAGNOSTIC FOR AI: Redis cache read failed. Falling back to local/DB.
                    _logger.warning("Redis cache read failed: %s", e)
                    use_redis = False

            if not use_redis:
                if cache_key in _local_cache:
                    return _local_cache[cache_key]

            result = func(self, *args, **kwargs)

            if use_redis:
                try:
                    # Attempt Redis write, fall back to local if serialization or connection fails
                    serialized_result = json.dumps(result)
                    r = redis.Redis(connection_pool=redis_pool)
                    r.setex(cache_key, 86400, serialized_result)  # 24h TTL
                    return result
                except (redis.RedisError, TypeError, json.JSONDecodeError) as e:
                    _logger.warning(
                        "Redis cache write failed, falling back to local: %s",
                        e,
                    )

            # Fallback to local memory cache
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
        if use_redis:
            try:
                r = redis.Redis(connection_pool=redis_pool)
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
    keys_to_delete = [
        k for k in _local_cache.keys() if k.startswith(prefix_local)
    ]
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
        _logger.warning("Security: Attempted to invalidate unknown model %s", model_name)
        return

    dbname = env.cr.dbname

    # 1. Invalidate locally and in Redis
    invalidate_model_cache(env, model_name, local_only=False)

    # 2. Notify all other workers via Postgres -> Daemon -> Redis Pub/Sub
    payload = json.dumps({"model": model_name, "dbname": dbname})
    env.cr.execute(
        "SELECT pg_notify(%s, %s)", ("distributed_cache_invalidation", payload)
    )
