<!-- Copyright © Bruce Perens K6BP. AGPL-3.0-or-later. -->

# Story: Distributed Cache Decoration

The `distributed_redis_cache` module provides a specialized decorator to ensure cache coherence across multiple Odoo workers.

## The Problem
Standard Odoo caching (`@tools.ormcache`) stores data in the local memory of a single WSGI worker. In a multi-worker or multi-node environment, when data changes, one worker might clear its cache while others continue to serve stale data.

## The Solution
By using the `@distributed_cache()` decorator (`COMM_distributed_cache_decorator`), developers can ensure that cached data is stored in a central Redis instance (`COMM_redis_connection_pool`).

## Key Features
- **Key Generation**: Cache keys ([@ANCHOR: COMM_distributed_cache_key_generation]) are automatically generated based on serialized function arguments using SHA256, ensuring unique and collision-resistant storage.
- **Fail-Open Resilience**: If Redis becomes unavailable, the system automatically falls back to a local memory cache, ensuring the application remains functional even if the cache cluster is down.
- **Testing Parity**: The decorator does not disable Redis during Odoo tests, so tests take the same path as production; a test that needs isolation passes `redis_bypass_cache=True` in the context.

## Anchor reference

Full entries for the anchors named above.

### `COMM_distributed_cache_decorator` [@ANCHOR: COMM_distributed_cache_decorator]

**Purpose.** Caches the result of an Odoo model method in two levels: a per-process in-memory cache (L1) and the shared Redis store (L2), keyed so that tenants never see each other's results. It replaces `@tools.ormcache`, which only knows about one worker.

**Call interface.** `distributed_cache()` takes no arguments and returns the decorator, so it is always written with parentheses. The decorated function must be a method of an Odoo model (the wrapper reads `self.env` and `self._name`); it may take any positional and keyword arguments and keeps its own name and docstring. The wrapper does this, in order:

| Step | What happens |
|---|---|
| Bypass | If `redis_bypass_cache` is truthy in the context, the method is called directly and nothing is read or written. |
| Key | `<dbname>:distributed_cache:<model>:<method>:w<website_id>:c<company ids>:<argument hash>`. `website_id` is the context's `website_id` or 0; the company part is the sorted `allowed_company_ids` from the context, else the sorted ids of `env.companies`, else `0`; the hash is `_get_hash` (see `COMM_distributed_cache_key_generation`) and includes `self`. |
| L1 | The process-wide `_local_cache` (LRU, 8192 entries, guarded by `LRU_LOCK`) is checked first; a hit returns the stored object itself, not a copy. |
| L2 | With an HMAC key (`_cache_hmac_key` is not `None`) and a Redis pool, the key is read, the HMAC verified, the payload unpickled and copied into L1. |
| Miss | The method runs; the result is pickled, signed and written with `SETEX` for 86400 seconds (24 hours), and is always stored in L1. |

**Returns and errors.** The method's own return value. An exception raised by the method propagates and nothing is cached. A `redis.RedisError` is logged as a warning and the call carries on without Redis (fail open). A payload that is malformed, fails the HMAC check or will not unpickle is logged and ignored, and Redis is not written for that call. A result that cannot be pickled is returned and kept in L1 only.

**Special concerns.** Synchronous and blocking: a Redis call waits at most the pool's 1 second socket timeout. The only lock taken is `LRU_LOCK`, briefly and never across a Redis call or the method itself. Callers share the cached object, so they must not mutate a returned list or dict, and a method whose result depends on something that is not in the key (the user, the language, the time) must not be decorated. L1 entries have no expiry of their own: they go when `poll_and_clear_local_cache` sees the invalidation counter change, when `invalidate_model_cache` runs for the model, or when the process restarts. Redis entries live 24 hours. Pickle is only read back after the HMAC verified, so a writer without the application secret cannot plant code.

**See also.** Test: `tests/test_distributed_cache.py` (cites this anchor); related `COMM_notify_model_invalidation_logic`, `COMM_invalidate_model_cache_logic`, `distributed_redis_cache:COMM_get_redis_connection`, `distributed_redis_cache:COMM_cache_hmac_key`.

### `COMM_redis_connection_pool` [@ANCHOR: COMM_redis_connection_pool]

**Purpose.** The module-level Redis settings and the shared connection pool in `redis_pool.py`. They are what every Odoo worker uses when no database-specific settings override them, and what `get_redis_connection` falls back to.

**Definition.** Read once when the module is imported:

| Name | Source | Default / meaning |
|---|---|---|
| `REDIS_HOST_DEFAULT` | environment `REDIS_HOST` | `redis` |
| `REDIS_PORT_DEFAULT` | environment `REDIS_PORT`, as `int` | 6379 |
| `REDIS_PASS_DEFAULT` | environment `REDIS_PASSWORD` | none (a test box needs no AUTH) |
| `REDIS_USERNAME_DEFAULT` | environment `REDIS_USERNAME` | none; production uses an ACL user from `/opt/hams/etc/redis.env` |
| `REDIS_DB_DEFAULT` | environment `REDIS_DB`, as `int` | 0 |
| `redis_pool` | `redis.ConnectionPool` built from the above | `decode_responses=True`, 1.0 s connect and socket timeouts |

**Invariants and concurrency.** The pool is created at import, opens connections lazily and is thread-safe; changing the environment needs a restart. Values saved on the Settings page do not change it: `get_redis_connection` builds separate pools for those. A Redis outage therefore shows up as a command error within 1 second, which the cache layer treats as "use local memory".

**See also.** Tests: `tests/test_distributed_cache.py`, `tests/test_fixes.py` (`COMM_test_redis_pool_env_variables`); function `distributed_redis_cache:COMM_get_redis_connection`.
