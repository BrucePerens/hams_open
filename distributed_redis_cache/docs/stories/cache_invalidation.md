# Story: Cross-Worker Cache Invalidation

Ensuring that all Odoo workers see the same data at the same time is critical for system integrity.

## The Pipeline
When a record is modified, the distributed cache uses a multi-stage invalidation pipeline:

1. **PostgreSQL NOTIFY**: The worker performing the change emits a `pg_notify` signal via the `notify_model_invalidation` function ([@ANCHOR: notify_model_invalidation_logic]).

2. **Daemon Relay**: A standalone `cache_manager.py` daemon ([@ANCHOR: cache_manager_redis_publish]) listens for these PostgreSQL signals and rebroadcasts them to Redis.
3. **Redis Pub/Sub**: All active Odoo workers are subscribed to a Redis invalidation channel.
4. **Middleware Interception**: The `ir.http` middleware ([@ANCHOR: redis_cache_interceptor]) in each worker checks for pending invalidations at the start of every request and clears its local memory as needed.

## Precise Invalidation
Unlike global cache clearing, the `invalidate_model_cache` function ([@ANCHOR: invalidate_model_cache_logic]) uses Redis `SCAN` to find and delete only the keys related to a specific model, preventing "cache stampedes" where the entire system slows down due to total cache loss.
