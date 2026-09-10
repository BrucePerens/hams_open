<!-- Copyright © Bruce Perens K6BP. AGPL-3.0-or-later. -->

# Journey: Invalidation Pipeline

This journey follows an invalidation signal as it travels through the system to ensure cluster-wide coherence.

1. **Trigger**: An administrator manually triggers an invalidation ([@ANCHOR: COMM_manual_cache_invalidation]) or a model mutation occurs.
2. **PostgreSQL Signal**: Odoo executes a `SELECT pg_notify(...)` to alert the database cluster.
3. **Daemon Reception**: The standalone `cache_manager.py` daemon, listening on the PostgreSQL channel, receives the notification.
4. **Redis Broadcast**: The daemon publishes the invalidation payload to the Redis `odoo_cache_invalidation_bus` ([@ANCHOR: COMM_cache_manager_redis_publish]).

5. **Counter Bump**: The same Redis pipeline that publishes the payload above also `INCR`s a single global `global_cache_invalidation_counter` key.
6. **Request-Time Poll (not a subscription)**: Corrected 2026-09-09 -- this step used to describe a background listener thread reading the Redis Pub/Sub bus and queueing per-model names; no such thread, queue, or subscription exists anywhere in this module's actual code (confirmed by reading `ir.http._authenticate` -- [@ANCHOR: COMM_redis_cache_interceptor] -- and grepping the whole codebase for any Redis `pubsub()`/`subscribe()` call outside the test suite; there is none). What each worker actually does, once per incoming HTTP request (skipped during `-i`/`-u`/`--stop-after-init`): a synchronous `GET global_cache_invalidation_counter`. If the value differs from what this worker process last saw, it clears its ENTIRE local L1 `_local_cache` (every model, not just the one that changed) and remembers the new counter value.
7. **Middleware Flush**: The full local-cache clear from step 6 happens directly inside `ir.http._authenticate`, before the request proceeds -- there is no separate queue-draining step. Note this means the finer-grained, per-model `invalidate_model_cache` ([@ANCHOR: COMM_invalidate_model_cache_logic]) SCAN/DELETE only ever runs synchronously, in the SAME worker/process that made the write (see `notify_model_invalidation`'s own postcommit hook) -- it is not what keeps OTHER workers' local caches correct. Those rely entirely on the blunter counter-triggered full clear in step 6, which is safe (no stale reads ever survive it) but not targeted: any model's invalidation anywhere in the cluster cold-starts every OTHER worker's local cache for every model, not just the one that changed. The Redis L2 store itself is unaffected by this -- `invalidate_model_cache`'s targeted SCAN/DELETE there is real and precise; only the LOCAL, in-process LRU is cleared in bulk on other workers.
