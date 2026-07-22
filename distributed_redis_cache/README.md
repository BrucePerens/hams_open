# Distributed Redis Cache (`distributed_redis_cache`)

*Copyright © Bruce Perens K6BP. Licensed under the GNU Affero General Public License v3.0 (AGPL-3.0).*

The Distributed Redis Cache module completely replaces Odoo's internal ORM cache with a high-performance, distributed Redis backend. This enables true horizontal scaling for Odoo Community by enforcing phase coherence across multiple WSGI workers and completely separate physical web servers.

## 🛠️ Dependencies & Installation

Because this is a standalone Open Source module that spawns external Python daemons, you must ensure its system dependencies are satisfied before installing it in your database. The module will fail-fast at startup if these are missing.

**External Dependencies:** `python-dotenv`

**Required Python Modules:** `redis`, `asyncpg`, `python-dotenv`

* **Debian/Ubuntu Installation:**
    ```bash
    sudo apt-get install python3-redis python3-asyncpg
    ```
* **Pip Installation:**
    ```bash
    pip3 install redis asyncpg
    ```

**System Requirements:**
* A running `redis-server` instance.

## 🌟 Key Features

* **Phase Coherence:** Uses PostgreSQL `NOTIFY` and a lightweight Python daemon (`cache_manager.py`) to broadcast cache invalidation events to all active nodes.
* **Granular Invalidation:** Replaces Odoo's heavy-handed `clear_caches()` with surgical, database-specific registry resets.
* **Control Panel:** Provides a backend UI to monitor Redis connection health, measure cache bloat, and manually flush the global cache if needed.
* **Zero-Sudo Security:** The IPC daemon authenticates via the `daemon_key_manager` and runs without elevated host privileges. [@ANCHOR: COMM_story_zero_sudo_cache_manager]

## 🚀 Getting Started

1. Ensure the Python dependencies (`redis`, `asyncpg`) are installed on your host.
2. Install the `distributed_redis_cache` module from the Odoo Apps menu.
3. Configure Redis settings:
   * Navigate to **Settings > Technical > Distributed Redis Cache** (or use the Distributed Cache Manager UI). [@ANCHOR: COMM_distributed_cache_settings_view]
   * Enter your Redis host, port, and password. These are stored securely in `ir.config_parameter`.
4. Start the background sync daemon:
   * Execute `daemons/cache_manager.py` as a systemd service.
   * The daemon requires read access to `/opt/hams/etc/keys/cache_manager.env` (managed automatically by the Daemon Key Manager module).
   * The daemon will use configurations from the environment file if available, or fall back to standard defaults.
5. Navigate to **Settings > Technical > Distributed Cache** in Odoo to verify the connection status. [@ANCHOR: COMM_distributed_cache_view]

## 🏗️ Architecture (CQRS)

This module implements a strict CQRS (Command Query Responsibility Segregation) pattern to bypass Odoo's single-threaded limitations:
1. When an Odoo worker invalidates the registry (e.g., changing a View or installing a module), it calls `self.env.cr.execute("NOTIFY distributed_cache_invalidation, ...")`.
2. The standalone `cache_manager.py` daemon, utilizing `asyncpg`, instantly catches the PostgreSQL notification.
3. The daemon broadcasts the invalidation payload to the central Redis Pub/Sub bus.
4. All active Odoo WSGI workers are subscribed to this Redis bus and immediately flush their local in-memory LRU caches upon receiving the broadcast, guaranteeing all web workers serve the exact same synchronized code and view state.

## 💻 Developer & AI Usage Guide

This module provides essential tools to integrate customized models into the distributed caching ecosystem safely. When developing new modules, always use the functions detailed below instead of standard Odoo caching (`@tools.ormcache`).

### 1. The `@distributed_cache()` Decorator
Replaces Odoo's `@tools.ormcache` to provide precise, cross-worker distributed caching with multi-tenant awareness (website and company boundaries). [@ANCHOR: COMM_distributed_cache_decorator]

**Usage:**
```python
from odoo.addons.distributed_redis_cache.redis_cache import distributed_cache
from odoo import models

class MyModel(models.Model):
    _name = 'my.model'

    @distributed_cache()
    def my_expensive_computation(self, param1):
        # Result will be cached in Redis and local memory across all workers
        return ...
```

### 2. `notify_model_invalidation(env, model_name)`
Triggers a cross-worker invalidation signal via PostgreSQL `NOTIFY`. Use this when you update records that are aggressively cached and need immediate propagation. [@ANCHOR: COMM_notify_model_invalidation_logic]

**Usage:**
```python
from odoo.addons.distributed_redis_cache.redis_cache import notify_model_invalidation

def invalidate_my_model(self):
    # Sends a PG NOTIFY to invalidate 'my.model' caches globally
    notify_model_invalidation(self.env, 'my.model')
```

### 3. `invalidate_model_cache(env, model_name, local_only=False)`
Flushes the cache specifically for the given model locally, and optionally within the Redis store. This is usually triggered automatically via daemon events but can be used programmatically. [@ANCHOR: COMM_manual_cache_invalidation]

**Usage:**
```python
from odoo.addons.distributed_redis_cache.redis_cache import invalidate_model_cache

def force_clear(self):
    # Clears local and Redis cache synchronously for 'my.model'
    invalidate_model_cache(self.env, 'my.model')
```

**Testing Note:** The module disables test bypass for caching to ensure parity with production. If you encounter test pollution, pass the `.with_context(redis_bypass_cache=True)` context variable during your environment operations. The system uses the Redis Cache Interceptor to safely bypass standard caching pathways when this flag is active. [@ANCHOR: COMM_redis_cache_interceptor]
