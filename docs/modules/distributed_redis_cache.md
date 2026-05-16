# ⚡ Distributed Redis Cache (`distributed_redis_cache`)

*Copyright © Bruce Perens K6BP. AGPL-3.0.*

<system_role>
**Context:** Technical documentation strictly for LLMs and Integrators. Use this to build dependent modules without needing the source code.
</system_role>

<architecture>
## 1. Architecture & Overview
Standard Odoo `@tools.ormcache` relies on a local worker registry cache, which can drift out of sync in horizontally scaled, multi-node environments. This module provides a fine-grained, distributed Redis-backed cache that enforces strict phase coherence across the entire cluster.

**The Invalidation Pipeline:**
1. An Odoo worker mutates a cached model and fires a PostgreSQL `NOTIFY` on the `distributed_cache_invalidation` channel.
2. The standalone `cache_manager.py` daemon receives the `NOTIFY` and publishes the payload to the Redis `odoo_cache_invalidation_bus` pub/sub channel. [@ANCHOR: cache_manager_redis_publish]
3. A background thread running inside every Odoo WSGI worker's `ir.http` middleware intercepts the Redis broadcast and instantly queues the model for local cache flushing before serving its next HTTP request. [@ANCHOR: redis_cache_interceptor]
</architecture>

<resilience>
## 2. Resilience (Fail-Open)
If the Redis server crashes or the `redis` Python module is uninstalled, the cache gracefully falls back to a standard Python dictionary (`_local_cache`). It will continue to function without crashing the web workers, though multi-node phase coherence will be temporarily lost until Redis is restored.
</resilience>

<api>
## 3. Application Programming Interface (API)

```python
from odoo.addons.distributed_redis_cache.redis_cache import distributed_cache, invalidate_model_cache
```

* **`@distributed_cache()`**: Use this decorator on `api.model` functions to automatically generate HMAC-SHA256 cache keys based on serialized arguments and write them to Redis with a 24h TTL.
* **`invalidate_model_cache(env, model_name)`**: Use this when overriding `.write()` or `.unlink()` to forcibly flush local WSGI memory before executing the `pg_notify` cross-worker alert.
</api>

<ui>
## 4. UI: Distributed Cache View [@ANCHOR: distributed_cache_view]
The module provides a UI to manage the cache and check Redis status.
</ui>

<config>
## 5. Configuration [@ANCHOR: cache_manager_config]
The daemon and Odoo worker can be configured via environment variables:
* **`REDIS_HOST`**: Redis server hostname (default: `redis` or `127.0.0.1`).
* **`REDIS_PORT`**: Redis server port (default: `6379`).
</config>

<stories_and_journeys>
## 6. Architectural Stories & Journeys

### Stories
* [Distributed Cache Decoration](distributed_redis_cache/docs/stories/cache_decoration.md)
* [Cross-Worker Cache Invalidation](distributed_redis_cache/docs/stories/cache_invalidation.md)
* [Manual Cache Management](distributed_redis_cache/docs/stories/manual_management.md)
* [System Resilience](distributed_redis_cache/docs/stories/resilience.md)

### Journeys
* [Daemon Operations](distributed_redis_cache/docs/journeys/daemon_operations.md)
* [Invalidation Pipeline](distributed_redis_cache/docs/journeys/invalidation_pipeline.md)
* [Request Caching Lifecycle](distributed_redis_cache/docs/journeys/request_caching_lifecycle.md)
</stories_and_journeys>
