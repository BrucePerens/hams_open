<!-- Copyright © Bruce Perens K6BP. AGPL-3.0-or-later. -->

# Story: Cross-Worker Cache Invalidation

Ensuring that all Odoo workers see the same data at the same time is critical for system integrity.

## The Pipeline
When a record is modified, the distributed cache uses a multi-stage invalidation pipeline:

1. **PostgreSQL NOTIFY**: The worker performing the change emits a `pg_notify` signal via the `notify_model_invalidation` function ([@ANCHOR: COMM_notify_model_invalidation_logic]).

2. **Daemon Relay**: A standalone `cache_manager.py` daemon ([@ANCHOR: COMM_cache_manager_redis_publish]) listens for these PostgreSQL signals, rebroadcasts the payload to a Redis Pub/Sub channel, and `INCR`s a single global counter key in the same pipeline.
3. **Request-Time Poll, not Pub/Sub** (corrected 2026-09-09 -- no Odoo worker actually subscribes to that Pub/Sub channel; it exists only for the daemon's own test suite to verify against). Each worker instead does a synchronous `GET` of the counter key at the start of every request.
4. **Middleware Interception**: The `ir.http` middleware ([@ANCHOR: COMM_redis_cache_interceptor]) in each worker compares the polled counter against the last value it saw; if it changed, it clears its ENTIRE local L1 memory (every model, not just the one that actually changed) before the request proceeds.

## Precise Invalidation
The `invalidate_model_cache` function ([@ANCHOR: COMM_invalidate_model_cache_logic]) uses Redis `SCAN` to find and delete only the Redis L2 keys related to a specific model, preventing "cache stampedes" where the entire system slows down due to total cache loss -- but this precision applies only to Redis and to the ONE worker that performed the write (via `notify_model_invalidation`'s own postcommit hook, which calls it directly, synchronously, in-process). Every OTHER worker in the cluster learns about the change only via the blunter global counter above and responds by dropping its entire local cache, not just the affected model's entries -- safe (no worker can ever serve a stale value past its next request) but not itself "targeted" the way this section's own title implies for the cluster as a whole.
