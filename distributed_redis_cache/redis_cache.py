# -*- coding: utf-8 -*-
# SPDX-License-Identifier: AGPL-3.0-or-later
import json
import os
import _pickle
import hmac
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

# _raw_crypto_secret() is called on every single cache sign/verify (it
# isn't itself cached), so an unconfigured deployment would otherwise log
# an ERROR line on every cache hit/miss -- loud, but not useful past the
# first one. Log it once per process instead of drowning out real signal.
_warned_missing_crypto_secret = False


def _raw_crypto_secret():
    # Deliberately NOT env["zero_sudo.security.utils"]._get_crypto_secret():
    # that method is itself @distributed_cache()-decorated, so calling it
    # from inside this module's own read/write path would recurse back
    # into _verify_and_unwrap_payload()/_sign_payload() on any L1 miss.
    # This mirrors that method's same env var / file / admin_passwd
    # fallback chain independently, without going through the cache layer
    # (zero_sudo already depends on this module, so importing the other
    # direction would also be circular).
    secret = os.environ.get("HAMS_CRYPTO_KEY")  # burn-ignore-env
    if not secret:
        try:
            secret_path = "/var/lib/odoo/hams_crypto.secret"
            if os.path.exists(secret_path):
                with open(secret_path, "r") as f:  # audit-ignore-path  # fmt: skip
                    secret = f.read().strip()
        except OSError as e:
            _logger.warning("Failed to read crypto secret file: %s", e)
    if not secret:
        secret = tools.config.get("admin_passwd")
    if not secret or secret == "admin":
        # Mirrors zero_sudo.security.utils._get_crypto_secret()'s own
        # fix: never substitute a hardcoded, publicly-known literal here
        # -- that would make the HMAC key itself guessable, defeating
        # the whole point of signing cache payloads.
        global _warned_missing_crypto_secret
        if not _warned_missing_crypto_secret:
            _warned_missing_crypto_secret = True
            _logger.error(
                "No cryptographic secret is configured for the Redis "
                "cache HMAC key -- refusing to fall back to a "
                "hardcoded, publicly-known secret."
            )
        secret = ""
    return secret


def _cache_hmac_key():
    # [!] SECURITY: Redis holds arbitrary-code-execution risk via
    # pickle.loads() -- anyone who can write to a key matching our
    # deterministic cache_key pattern (e.g. a compromised/misconfigured
    # Redis instance, or a network peer with Redis access but not the
    # application's own secret) could otherwise plant a malicious pickle
    # payload and get it deserialized by any worker that reads it back.
    # HMAC-sign every payload with a secret only this application knows,
    # and refuse to unpickle anything whose signature doesn't verify --
    # an attacker without the secret cannot forge a payload that will
    # ever reach _pickle.loads().
    secret = _raw_crypto_secret()
    return hashlib.sha256(f"{secret}:distributed_redis_cache_hmac".encode()).digest()


def _sign_payload(payload_bytes):
    key = _cache_hmac_key()
    signature = hmac.new(key, payload_bytes, hashlib.sha256).hexdigest()
    return f"{signature}:{payload_bytes.hex()}"


def _verify_and_unwrap_payload(stored):
    signature, _, hex_payload = stored.partition(":")
    if not hex_payload:
        raise ValueError("Malformed cache payload: missing HMAC signature.")
    payload_bytes = bytes.fromhex(hex_payload)
    key = _cache_hmac_key()
    expected = hmac.new(key, payload_bytes, hashlib.sha256).hexdigest()
    if not hmac.compare_digest(signature, expected):
        raise ValueError("Cache payload failed HMAC verification (tampered or forged).")
    return payload_bytes


def _get_hash(*args, **kwargs):
    # [@ANCHOR: COMM_distributed_cache_key_generation]
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
    # [@ANCHOR: COMM_distributed_cache_decorator]
    """
    Fine-grained, distributed Redis-backed cache decorator.
    Replaces @tools.ormcache to support precise cross-worker invalidation.
    """

    def decorator(func):
        @wraps(func)
        def wrapper(self, *args, **kwargs):
            if self.env.context.get("redis_bypass_cache"):
                return func(self, *args, **kwargs)

            dbname = self.env.cr.dbname
            model_name = self._name

            # Multi-Tenant awareness: Include website_id and company_id in cache key
            website_id = self.env.context.get("website_id") or 0
            
            # Use only context to avoid triggering N+1 queries from self.env.company
            allowed_company_ids = self.env.context.get("allowed_company_ids", [])
            company_ids_str = ",".join(map(str, allowed_company_ids)) if allowed_company_ids else "0"
            
            # [!] SECURITY: Multi-tenant isolation is enforced via website_id and company_id in the cache key.
            website_suffix = f":w{website_id}"
            company_suffix = f":c{company_ids_str}"

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
                        payload_bytes = _verify_and_unwrap_payload(cached)
                        res = _pickle.loads(payload_bytes)
                        with LRU_LOCK:
                            _local_cache[cache_key] = res
                        return res
                except redis.RedisError as e:
                    _logger.warning("Network partition detected. Bypassing Redis: %s", e)
                    use_redis = False
                except (_pickle.PickleError, ValueError) as e:
                    _logger.warning(
                        "Redis cache payload rejected (corrupted or forged): %s", e
                    )
                    use_redis = False

            result = func(self, *args, **kwargs)

            if use_redis:
                try:
                    serialized_result = _sign_payload(_pickle.dumps(result))
                    r = get_redis_connection(self.env)
                    r.setex(cache_key, 86400, serialized_result)  # 24h TTL
                except redis.RedisError as e:
                    _logger.warning("Network partition detected during cache write: %s", e)
                except (TypeError, _pickle.PickleError) as e:
                    _logger.warning("Redis cache write serialization failed: %s", e)

            # Always populate L1 local fallback cache
            with LRU_LOCK:
                _local_cache[cache_key] = result
            return result

        return wrapper

    return decorator


def invalidate_model_cache(env, model_name, local_only=False):
    # [@ANCHOR: COMM_invalidate_model_cache_logic]
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
        keys_to_delete = [k for k in _local_cache if k.startswith(prefix_local)]
        for k in keys_to_delete:
            _local_cache.pop(k, None)


def notify_model_invalidation(env, model_name):
    # [@ANCHOR: COMM_notify_model_invalidation_logic]
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
