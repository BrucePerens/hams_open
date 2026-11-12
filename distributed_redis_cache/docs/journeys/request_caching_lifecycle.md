<!-- Copyright © Bruce Perens K6BP. SPDX-License-Identifier: AGPL-3.0-or-later -->

# Journey: Request Caching Lifecycle

This journey describes the path of a data retrieval request when using the distributed cache.

1. **Function Call**: An Odoo business logic function decorated with `@distributed_cache()` (`COMM_distributed_cache_decorator`) is invoked.

2. **Key Generation**: The decorator serializes the function arguments and generates a unique SHA-256 hash (`COMM_distributed_cache_key_generation`).

3. **Redis Lookup**: The system attempts to fetch the value from the Redis connection pool ([@ANCHOR: COMM_redis_connection_pool]).
4. **Cache Hit**:
   - If the key exists in Redis, the signed payload is verified, unpickled and returned immediately.
5. **Cache Miss**:
   - If the key does not exist, the original function is executed to fetch data from the database.
   - The result is stored in Redis with a 24-hour TTL for future use.
6. **Fallback**:
   - If Redis is unreachable, the system checks the local memory fallback.
   - If not in memory, it fetches from the DB and stores the result locally.

## Anchor reference

Full entries for the anchors named in steps 1 and 2.

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

### `COMM_distributed_cache_key_generation` [@ANCHOR: COMM_distributed_cache_key_generation]

**Purpose.** Produces the stable argument digest that ends every cache key, so the same call hits the same entry on every worker and different calls never share one.

**Call interface.** `_get_hash(*args, **kwargs)` returns a 64 character lowercase hex `str`, the SHA-256 of `json.dumps([args, kwargs], sort_keys=True)` after each value is normalised. The decorator passes `self` first, so the recordset is part of the hash. It is a plain hash, not keyed: signing is done separately by the payload HMAC.

| Value | Becomes |
|---|---|
| Odoo recordset | `model(sorted ids)` |
| `date`, `datetime` | ISO 8601 string |
| list, tuple | list of normalised items (a tuple and a list hash alike) |
| dict | `{str(key): value}` in sorted key order |
| bytes | hex string |
| set, frozenset | list sorted by `str` |
| `None`, bool, int, float, str | itself |
| anything else | `str(obj)` (unknown objects with address-based reprs would give unstable keys; keep arguments to the types above) |

**Special concerns.** Pure, no locks or I/O. A date and its ISO string hash the same. Cost grows with argument size. Keyword arguments are order independent.

**See also.** Test `tests/test_distributed_cache.py`; used by `COMM_distributed_cache_decorator`.
