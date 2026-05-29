# Distributed Redis Cache (`distributed_redis_cache`)

*Copyright © Bruce Perens K6BP. Licensed under the GNU Affero General Public License v3.0 (AGPL-3.0).*

Fine-grained distributed caching and phase coherence for horizontally scaled Odoo clusters.

## Features
- **Distributed Redis-backed cache**: Replaces or augments Odoo's local cache for cluster-wide consistency across multiple servers.
- **Multi-Tenant Awareness**: Keeps data strictly separated by website and company, so users only see what they are supposed to.
- **No More Out-of-Sync Data**: Ensures all your Odoo servers show the same updated information at the same time.
- **Resilient by Design**: If your Redis server goes offline, Odoo won't crash. It will safely switch back to its standard way of working until Redis is back.
- **High Performance**: Only clears the specific data that changed, rather than dumping the whole cache. This keeps your system snappy even during updates.
- **Production Ready**: Efficiently manages millions of cache keys without slowing down your database or Redis server.
- **Easy Management UI**: Check system health and manually refresh data from a simple dashboard.
- **Secure**: Uses minimal permissions and dedicated service accounts for background tasks.

## Installation
1.  Ensure you have a Redis server running and accessible.
2.  Install the required Python libraries: `pip install redis asyncpg`.
3.  Install this module in your Odoo instance.

## Configuration
The module automatically looks for a Redis server at `127.0.0.1:6379`. You can change this using these environment variables:
- `REDIS_HOST`: The address of your Redis server (e.g., `10.0.0.5`).
- `REDIS_PORT`: The port number (default is `6379`).
- `REDIS_PASSWORD`: Your Redis password, if you have one.

## How It Works
1.  **Change Detected**: When you update something in Odoo, it sends a quick "ping" to the database.
2.  **Signal Relayed**: A small background program (the *Cache Manager*) sees this ping and tells the Redis server.
3.  **Cluster Update**: All your Odoo servers are listening to Redis. They hear the message and immediately update their local memory.
4.  **Fresh Data**: The next person to visit your site sees the latest information, no matter which server they connect to.

## Security
Built with the **Zero-Sudo** architecture. Operations are performed by dedicated service accounts with minimal privileges. The `cache_manager_sys` user handles daemon-to-database communication.

## Documentation
Comprehensive user documentation is available via the **Manual Library** module after installation.

---

# Technical Documentation

<system_role>
**Context:** Technical documentation strictly for LLMs and Integrators. Use this to build dependent modules without needing the source code.
</system_role>

<architecture>
## 1. Architecture & Overview
Standard Odoo `@tools.ormcache` relies on a local worker registry cache, which can drift out of sync in multi-node environments. This module provides a fine-grained, distributed Redis-backed cache enforcing strict phase coherence.

**The Invalidation Pipeline:**
1. An Odoo worker mutates a cached model and fires a PostgreSQL `NOTIFY` on the `distributed_cache_invalidation` channel.
2. The standalone `cache_manager.py` daemon receives the `NOTIFY`, validates the payload, and publishes to the Redis `odoo_cache_invalidation_bus` channel. [@ANCHOR: cache_manager_redis_publish]
3. A background thread in every Odoo worker's `ir.http` middleware intercepts the broadcast and queues the model for local flushing. [@ANCHOR: redis_cache_interceptor]
</architecture>

<resilience>
## 2. Resilience (Fail-Open)
If Redis is unreachable, the system gracefully falls back to a standard Python dictionary (`_local_cache`) limited to 8192 entries. It continues functioning without crashing, though cross-node coherence is temporarily lost. Background listeners handle connection drops gracefully.
</resilience>

<api>
## 3. Application Programming Interface (API)

```python
from odoo.addons.distributed_redis_cache.redis_cache import distributed_cache, invalidate_model_cache, notify_model_invalidation
```

* **`@distributed_cache()`**: Decorator for `api.model` functions. Generates SHA256 cache keys based on serialized arguments and writes to Redis with a 24h TTL. Handles `bytes`, `sets`, `frozensets`, and recordsets deterministically. **Multi-Tenant Aware**: Isolated keys if `website_id` or `company_id` are in context. [@ANCHOR: distributed_cache_decorator]
* **`invalidate_model_cache(env, model_name, local_only=False)`**: Forcibly flushes model cache. Uses batched `SCAN` for production safety. [@ANCHOR: invalidate_model_cache_logic]
* **`notify_model_invalidation(env, model_name)`**: Triggers cluster-wide invalidation signal via Postgres NOTIFY. [@ANCHOR: notify_model_invalidation_logic]
</api>

<ui>
## 4. UI: Distributed Cache View [@ANCHOR: distributed_cache_view]
Provides a management form to check Redis status and manually invalidate model caches. [@ANCHOR: manual_cache_invalidation] [@ANCHOR: check_redis_status_logic]
</ui>

<config>
## 5. Configuration [@ANCHOR: cache_manager_config]
Configurable via environment variables or `.env` file at `/var/lib/odoo/daemon_keys/cache_manager.env`.
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
* **Documentation Injection:** Provisions documentation into `manual.article` upon installation. [@ANCHOR: doc_inject_distributed_redis_cache]

### Zero-Sudo
* **Micro-Privilege Service Account:** Uses `cache_manager_sys` for daemon operations. [@ANCHOR: story_zero_sudo_cache_manager]
</stories_and_journeys>
