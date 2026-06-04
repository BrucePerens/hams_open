# Distributed Redis Cache (`distributed_redis_cache`)

*Copyright © Bruce Perens K6BP. Licensed under the GNU Affero General Public License v3.0 (AGPL-3.0).*

Synchronized, cluster-wide caching for Odoo servers to ensure all your web nodes see the same data instantly.

## Features
- **Shared Cluster Cache**: Synchronizes multiple Odoo servers by leveraging a central Redis-backed memory pool, ensuring a "single source of truth" across your infrastructure.
- **Instant Phase Coherence**: Emits real-time invalidation signals via PostgreSQL NOTIFY and Redis Pub/Sub, ensuring all nodes flush stale data the millisecond it changes.
- **Smart Isolation**: Automatically separates data between different websites and companies so information never leaks between clients.
- **Automatic Fallback**: If the shared memory server (Redis) goes offline, Odoo automatically switches back to its standard local memory so your site stays up.
- **Fast Cleanup**: Precisely clears only the data that changed, keeping the rest of the system running at full speed.
- **Management Dashboard**: A simple interface for administrators to check system health and manually refresh data if needed.
- **Secure by Design**: Uses restricted "service accounts" to perform background tasks, following the "Zero-Sudo" security principle.

## How it Works (Non-Technical)
Normally, when you have multiple Odoo servers running your website, each one has its own "short-term memory" (cache). If you update a product price on Server A, Server B might still remember the old price for a few minutes.

This module links all your servers to a single, high-speed shared memory (Redis). As soon as you change data anywhere, all servers get a "tap on the shoulder" and immediately refresh their memory.

## Configuration
Administrators can check connection status via the **Distributed Cache** menu in Odoo settings. The system is designed to "fail-open," meaning it will never crash your site if Redis is unavailable—it will simply work like a standard Odoo installation until the connection is restored.

### How to Use the Dashboard
1. Go to **Settings** > **Technical** > **Distributed Cache**.
2. Click **Check Redis Status** to verify the connection.
3. To manually clear the cache for a specific model (e.g., if you've done a bulk SQL import), select the model in the dropdown and click **Invalidate Cache**.

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

* **`@distributed_cache()`**: Decorator for `api.model` functions. Generates SHA256 cache keys based on serialized arguments and writes to Redis with a 24h TTL. Handles `bytes`, `sets`, `frozensets`, and recordsets deterministically. **Multi-Tenant Aware**: Isolated keys via `website_id` in context and `env.company.id`. [@ANCHOR: distributed_cache_decorator]
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
