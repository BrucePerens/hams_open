<!-- Copyright © Bruce Perens K6BP. AGPL-3.0-or-later. -->

# Journey: Invalidation Pipeline

This journey follows an invalidation signal as it travels through the system to ensure cluster-wide coherence.

Terms used below: **L1** is each worker process's own in-memory cache (`_local_cache`, a least-recently-used cache capped at 8192 entries); **L2** is the shared Redis store. See the Architecture section of [the module README](../../README.md).

1. **Trigger**: An administrator manually triggers an invalidation (`COMM_manual_cache_invalidation`) or a model mutation occurs.
2. **PostgreSQL Signal**: Odoo executes a `SELECT pg_notify(...)` to alert the database cluster.
3. **Daemon Reception**: The standalone `cache_manager.py` daemon, listening on the PostgreSQL channel, receives the notification.
4. **Redis Broadcast**: The daemon publishes the invalidation payload to the Redis `odoo_cache_invalidation_bus` (`COMM_cache_manager_redis_publish`).

5. **Counter Bump**: The same Redis pipeline that publishes the payload above also `INCR`s a single global `global_cache_invalidation_counter` key.
6. **Request-Time Poll (not a subscription)**: Corrected 2026-09-09 -- this step used to describe a background listener thread reading the Redis Pub/Sub bus and queueing per-model names; no such thread, queue, or subscription exists anywhere in this module's actual code (confirmed by reading `ir.http._authenticate` -- [@ANCHOR: COMM_redis_cache_interceptor] -- and grepping the whole codebase for any Redis `pubsub()`/`subscribe()` call outside the test suite; there is none). What each worker actually does, once per incoming HTTP request -- and, since the cron-dispatch override ([@ANCHOR: COMM_cron_cache_interceptor]), once per cron job too -- is a synchronous `GET global_cache_invalidation_counter`. It is skipped only while the registry is still being built ([@ANCHOR: COMM_should_poll_for_invalidation]), so that a module install or upgrade never depends on Redis. **Corrected 2026-09-15**: this used to read "skipped during `-i`/`-u`/`--stop-after-init`", which matched the code but described a gate that stayed shut for the entire lifetime of a server started with `-u` and then left serving -- Odoo 19 never clears those config flags after loading, so such a worker never polled again. The gate now asks the registry whether it has finished loading instead. If the value differs from what this worker process last saw, it clears its ENTIRE local L1 `_local_cache` (every model, not just the one that changed) and remembers the new counter value.
7. **Middleware Flush**: The full local-cache clear from step 6 happens directly inside `ir.http._authenticate`, before the request proceeds -- there is no separate queue-draining step. Note this means the finer-grained, per-model `invalidate_model_cache` ([@ANCHOR: COMM_invalidate_model_cache_logic]) SCAN/DELETE only ever runs synchronously, in the SAME worker/process that made the write (see `notify_model_invalidation`'s own postcommit hook) -- it is not what keeps OTHER workers' local caches correct. Those rely entirely on the blunter counter-triggered full clear in step 6, which is safe (no stale reads ever survive it) but not targeted: any model's invalidation anywhere in the cluster cold-starts every OTHER worker's local cache for every model, not just the one that changed. The Redis L2 store itself is unaffected by this -- `invalidate_model_cache`'s targeted SCAN/DELETE there is real and precise; only the LOCAL, in-process LRU is cleared in bulk on other workers.

## Anchor reference

Full entries for the anchors named in steps 1 and 4.

### `COMM_manual_cache_invalidation` [@ANCHOR: COMM_manual_cache_invalidation]

**Purpose.** The **Invalidate Model Cache** button of the Distributed Cache Manager form: lets an administrator flush one model's cached results on demand.

**Call interface.** `distributed.cache.config.action_invalidate_model_cache()` (a `@public_rpc` button method on a transient model; `public_rpc` is the `zero_sudo` marker that lists it as callable). It needs exactly one record (`ensure_one`); the record's `model_id` (Many2one to `ir.model`, required in the form) names the model. Returns an `ir.actions.client` `display_notification` dict (title "Success", message "Cache invalidated for model <model>") when a model is selected, otherwise `None` and nothing happens.

**Special concerns.** It does not clear anything itself: it calls `notify_model_invalidation(env, model)` (see `COMM_notify_model_invalidation_logic`), so the flush follows the normal pipeline and reaches the whole cluster after the transaction commits, not only the administrator's worker. The model name comes from the selected `ir.model` record, so it is always a real model. Access is limited by the form's menu group `distributed_redis_cache.group_distributed_cache_manager` and the model's access rules.

**See also.** Test `tests/test_distributed_cache.py`; view `COMM_distributed_cache_view`.

### `COMM_cache_manager_redis_publish` [@ANCHOR: COMM_cache_manager_redis_publish]

**Purpose.** The daemon's relay step: it takes one PostgreSQL notification payload and makes it visible to every Odoo worker through Redis.

**Call interface.** `async broadcast_to_redis(payload)` in `daemons/cache_manager.py`: `payload` is the raw string from `pg_notify`, expected to be a JSON object with non-empty `model` and `dbname`. Returns `None`. It returns at once when `redis_client` is not set. Anything that is not such an object is dropped with a warning; malformed JSON is logged as an error; any other failure (including Redis timeouts) is logged with a traceback and swallowed. Nothing is retried, so an invalidation lost to a Redis outage stays lost until the next one.

**Effects.** One pipeline (MULTI/EXEC) does `PUBLISH odoo_cache_invalidation_bus <payload>` and `INCR global_cache_invalidation_counter`. No Odoo worker subscribes to the channel (only the daemon's tests do); workers notice the counter change when they poll it. Runs as its own asyncio task per notification (see `distributed_redis_cache:COMM_postgres_notify_handler`); Redis calls time out after 10 seconds. Security: the payload is validated before use and never executed.

**See also.** Test `tests/test_cache_manager_real.py`; `distributed_redis_cache:COMM_poll_and_clear_local_cache` for the reader.
