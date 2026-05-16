# Distributed Redis Cache (`distributed_redis_cache`)

*Copyright © Bruce Perens K6BP. Licensed under the GNU Affero General Public License v3.0 (AGPL-3.0).*

Fine-grained distributed caching and phase coherence for horizontally scaled Odoo clusters.

## Features
- Distributed Redis-backed cache to replace/augment Odoo's local cache.
- Prevents cache drift across multiple Odoo nodes.
- Fail-open design: falls back to local memory if Redis is unavailable.
- Fine-grained invalidation: only flushes specific models, not the entire cache.
- Management UI for status checks and manual invalidation.
- **Zero-Sudo Architecture**: Background operations execute with minimal privileges using dedicated service accounts.

## Installation
This module requires a Redis server.
Ensure the `redis` and `asyncpg` Python packages are installed.

## Configuration
The following environment variables can be used to configure the Redis connection:
- `REDIS_HOST`: Defaults to `redis` or `127.0.0.1`.
- `REDIS_PORT`: Defaults to `6379`.
- `REDIS_PASSWORD`: Optional Redis password.

## Architecture
- **Postgres NOTIFY**: Triggered when a model's cache needs invalidation.
- **Cache Manager Daemon**: A standalone Python service that bridges Postgres NOTIFY to Redis Pub/Sub.
- **Redis Pub/Sub**: Distributes invalidation signals to all Odoo workers.
- **Middleware Interceptor**: Odoo workers check for signals in `ir.http` and flush local caches accordingly.

## Security
Built with the **Zero-Sudo** architecture. Operations are performed by dedicated service accounts with minimal privileges.

## Documentation
Comprehensive documentation is available via the **Manual Library** module after installation.

---

# Technical Documentation

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
from odoo.addons.distributed_redis_cache.redis_cache import distributed_cache, invalidate_model_cache, notify_model_invalidation
```

* **`@distributed_cache()`**: Use this decorator on `api.model` functions to automatically generate HMAC-SHA256 cache keys based on serialized arguments and write them to Redis with a 24h TTL.
* **`invalidate_model_cache(env, model_name, local_only=False)`**: Use this to forcibly flush local WSGI memory. If `local_only` is False, it also attempts to delete keys from Redis.
* **`notify_model_invalidation(env, model_name)`**: Use this to trigger a cross-worker invalidation signal via Postgres NOTIFY. [@ANCHOR: notify_model_invalidation_logic]
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
* **`REDIS_PASSWORD`**: Redis server password.
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

### Installation
* **Documentation Injection:** The module automatically provisions its documentation payload into the `knowledge.article` or `manual.article` API upon installation. [@ANCHOR: doc_inject_distributed_redis_cache]

### Zero-Sudo
* **Micro-Privilege Service Account:** The module uses `cache_manager_sys` for daemon operations. [@ANCHOR: story_zero_sudo_cache_manager]
</stories_and_journeys>
