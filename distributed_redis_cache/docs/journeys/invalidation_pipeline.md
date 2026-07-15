<!-- Copyright © Bruce Perens K6BP. AGPL-3.0. -->

# Journey: Invalidation Pipeline

This journey follows an invalidation signal as it travels through the system to ensure cluster-wide coherence.

1. **Trigger**: An administrator manually triggers an invalidation ([@ANCHOR: COMM_manual_cache_invalidation]) or a model mutation occurs.
2. **PostgreSQL Signal**: Odoo executes a `SELECT pg_notify(...)` to alert the database cluster.
3. **Daemon Reception**: The standalone `cache_manager.py` daemon, listening on the PostgreSQL channel, receives the notification.
4. **Redis Broadcast**: The daemon publishes the invalidation payload to the Redis `odoo_cache_invalidation_bus` ([@ANCHOR: COMM_cache_manager_redis_publish]).

5. **Worker Subscription**: Every Odoo worker runs a background listener thread ([@ANCHOR: COMM_redis_cache_interceptor]) that picks up the Redis message.
6. **Local Queueing**: The listener thread adds the model name to a thread-safe `_invalidation_queue`.
7. **Middleware Flush**: When a worker starts processing its next HTTP request, the `ir.http` middleware checks the queue and invokes `invalidate_model_cache` ([@ANCHOR: COMM_invalidate_model_cache_logic]) to clear the relevant memory before the request proceeds.
