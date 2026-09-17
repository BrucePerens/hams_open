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
    clear_db_config_cache,
)
import threading
from odoo.tools.lru import LRU

_logger = logging.getLogger(__name__)

# Local fallback cache to maintain HA if Redis is unreachable.
# Limit to 8192 entries to prevent memory exhaustion during Redis outages.
_local_cache = LRU(8192)
LRU_LOCK = threading.Lock()

# Shared, process-wide "last seen" invalidation counter for
# poll_and_clear_local_cache() below -- deliberately a single module-level
# value rather than one per calling model (ir.http, ir.cron), so the two
# call sites can never each decide independently and redundantly clear the
# cache on the same counter change; whichever call site polls first wins,
# and the other sees latest == last_cache_counter and skips.
_last_cache_counter = None
_LAST_CACHE_COUNTER_LOCK = threading.Lock()


# [@ANCHOR: distributed_redis_cache:COMM_poll_and_clear_local_cache]
def poll_and_clear_local_cache(env):
    """Poll Redis's global invalidation counter and clear the process-local L1
    cache if it changed since the last poll from any call site.

    This is the ONLY thing that ever clears `_local_cache`, and (see
    `night_shift_todo/low/misc-small-relay-and-infra-cleanups-1487fd74.md`) the
    ONLY thing that clears `redis_pool._db_configs` on a worker OTHER than the
    one that saved new Redis settings -- `res_config_settings.set_values()`
    bumps this same counter via `pg_notify`/`cache_manager.py` for exactly this
    reason. It must run from every code path that can call a
    `@distributed_cache()`-decorated method outside of a fresh process start,
    or that path's L1 entries never expire until the process itself recycles.
    `ir.http._authenticate` (the request path) is one such call site;
    `ir.cron._process_job` (the cron-dispatch path, which reaches
    `@distributed_cache()`-decorated code -- e.g. `cloudflare`'s purge-queue
    cron -- without ever going through `_authenticate`) is the other.
    """
    global _last_cache_counter
    try:
        r = get_redis_connection(env)
        latest = r.get("global_cache_invalidation_counter")
        with _LAST_CACHE_COUNTER_LOCK:
            if latest and latest != _last_cache_counter:
                with LRU_LOCK:
                    _local_cache.clear()
                clear_db_config_cache(env.cr.dbname)
                _last_cache_counter = latest
    except redis.RedisError as e:
        _logger.warning("Failed to execute stateless Redis poll: %s", e)


# [@ANCHOR: distributed_redis_cache:COMM_should_poll_for_invalidation]
def should_poll_for_invalidation(registry):
    """Whether this process should poll Redis for cache invalidation right now.

    The poll must NOT run while a registry is still being built: module install and upgrade
    (`-i`/`-u`) load a half-constructed registry, and letting that depend on Redis would turn an
    unreachable cache into a failed upgrade. It MUST run for the entire life of a process that is
    actually serving, which is the half that was broken.

    This deliberately does not consult `tools.config`'s `init`/`update`/`stop_after_init` flags,
    which is what both poll sites used to do. Odoo 19 never clears them: `odoo/tools/config.py`
    sets them from the command line, and nothing in `odoo/modules/loading.py`,
    `odoo/orm/registry.py`, `odoo/service/server.py` or `odoo/cli/server.py` resets them once
    loading finishes (older Odoo reset `tools.config[kind] = {}` at the end of `load_modules`;
    this version does not). So an Odoo started as `odoo -u some_module` that then went on to serve
    -- the common one-step "deploy the upgrade and restart" -- kept `update` truthy for its whole
    lifetime, and every HTTP worker and cron worker in it served its process-local L1
    `_local_cache` forever without ever reading `global_cache_invalidation_counter`. That is
    indefinitely stale cached data on a live server, and it is the bug this gate replaces.

    `registry.ready` asks the question those flags were only ever standing in for. `Registry.new()`
    sets it True only after `load_modules()` has returned, and `Registry.init()` sets it False
    again for a reload, so it is False for exactly the window that needed protecting -- however the
    process was started.

    A test process is deliberately NOT excluded. `post_install` suites run against an already-ready
    registry, so they poll exactly as a serving worker does; that is the point. Making this gate
    probe `test_enable` would be the test-evasion pattern `check_burn_list.py` forbids outright,
    and would leave the production path untested by construction.
    """
    return registry is not None and registry.ready


# _raw_crypto_secret() is called on every single cache sign/verify (it
# isn't itself cached), so an unconfigured deployment would otherwise log
# an ERROR line on every cache hit/miss -- loud, but not useful past the
# first one. Log it once per process instead of drowning out real signal.
_warned_missing_crypto_secret = False


# [@ANCHOR: distributed_redis_cache:COMM_raw_crypto_secret]
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


# [@ANCHOR: distributed_redis_cache:COMM_cache_hmac_key]
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
    #
    # Returns None when no real secret is configured. Every unconfigured
    # deployment would otherwise derive the exact same key from the
    # empty string, which is exactly as guessable as the hardcoded
    # literal _raw_crypto_secret()'s own docstring says it refuses to
    # fall back to -- anyone who reads this open-source file already
    # knows it. Callers MUST treat None as "do not touch Redis for this
    # payload", not sign or verify with it.
    secret = _raw_crypto_secret()
    if not secret:
        return None
    return hashlib.sha256(f"{secret}:distributed_redis_cache_hmac".encode()).digest()


# [@ANCHOR: distributed_redis_cache:COMM_sign_payload]
def _sign_payload(key, payload_bytes):
    signature = hmac.new(key, payload_bytes, hashlib.sha256).hexdigest()
    return f"{signature}:{payload_bytes.hex()}"


# [@ANCHOR: distributed_redis_cache:COMM_verify_and_unwrap_payload]
def _verify_and_unwrap_payload(key, stored):
    signature, _, hex_payload = stored.partition(":")
    if not hex_payload:
        raise ValueError("Malformed cache payload: missing HMAC signature.")
    payload_bytes = bytes.fromhex(hex_payload)
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

            # bug-hunt (2026-09-09): this used to fall back to a fixed
            # literal "0" whenever "allowed_company_ids" was absent from
            # context -- which it is for the overwhelming majority of
            # calls (any backend RPC, cron job, or controller call that
            # didn't go through the web client's own company switcher).
            # THE REAL active company scope in that case is NOT "company
            # 0" (no such company exists) -- Environment.companies
            # (odoo/orm/environments.py) falls back to
            # self.user._get_company_ids() precisely when
            # "allowed_company_ids" isn't in context. Using a constant
            # instead meant two different users in two different
            # companies, both calling the same @distributed_cache()'d
            # method with no explicit company context, got the exact
            # same cache key suffix regardless of which company either
            # of them actually belonged to -- silently defeating the
            # "[!] SECURITY" comment below for any future decorated
            # function whose own result varies by company through
            # ambient scope alone (no company-varying argument, no
            # company-scoped recordset in `self`). Not found to be
            # exploitable against any CURRENT caller (every @distributed_
            # cache()'d function reviewed either returns server-wide,
            # non-tenant-scoped data, or already takes a
            # website_id/record whose own ids disambiguate the arg_hash) --
            # but the security invariant this comment states should be
            # true regardless of what happens to call it today. Derive
            # the real fallback from Environment.companies (sorted, so
            # [1,2] and [2,1] hash identically) instead of a dummy
            # constant; also sort the explicit-context path for the same
            # reason (unsorted, [1,2] and [2,1] context values previously
            # produced two different cache keys for the same real scope).
            allowed_company_ids = self.env.context.get("allowed_company_ids")
            if allowed_company_ids:
                company_ids_str = ",".join(map(str, sorted(allowed_company_ids)))
            else:
                company_ids_str = ",".join(map(str, sorted(self.env.companies.ids))) or "0"

            # [!] SECURITY: Multi-tenant isolation is enforced via website_id and company_id in the cache key.
            website_suffix = f":w{website_id}"
            company_suffix = f":c{company_ids_str}"

            arg_hash = _get_hash(self, *args, **kwargs)
            cache_key = f"{dbname}:distributed_cache:{model_name}:{func.__name__}{website_suffix}{company_suffix}:{arg_hash}"

            # L1 Cache Check (In-Memory)
            with LRU_LOCK:
                if cache_key in _local_cache:
                    return _local_cache[cache_key]

            # [!] SECURITY: hmac_key is None on any deployment with no
            # configured crypto secret. Redis is skipped entirely in that
            # case (falling back to the L1 local cache below) rather than
            # signing/verifying with a key every reader of this file
            # could derive themselves -- see _cache_hmac_key()'s own
            # docstring. This degrades to per-process caching, not to an
            # insecure Redis cache.
            hmac_key = _cache_hmac_key()
            use_redis = bool(redis and redis_pool) and hmac_key is not None

            # Ensure Redis is running in testing environments to maintain production parity.

            if use_redis:
                try:
                    r = get_redis_connection(self.env)
                    cached = r.get(cache_key)
                    if cached:
                        # [!] DIAGNOSTIC FOR AI: Redis cache hit for %s
                        _logger.debug("Redis cache hit: %s", cache_key)
                        payload_bytes = _verify_and_unwrap_payload(hmac_key, cached)
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
                    serialized_result = _sign_payload(hmac_key, _pickle.dumps(result))
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
