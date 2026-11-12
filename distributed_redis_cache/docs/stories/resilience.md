<!-- Copyright © Bruce Perens K6BP. AGPL-3.0-or-later. -->

# Story: System Resilience and Fail-Open

The `distributed_redis_cache` module is designed with a "safety-first" architecture to ensure that cache infrastructure failures do not result in application downtime.

## Graceful Degradation
The module continuously monitors the health of the Redis connection ([@ANCHOR: COMM_redis_connection_pool]). If the Redis server crashes or the network is interrupted:
- The `@distributed_cache` decorator catches the exception.
- It logs a debug message.
- It switches to a local `_local_cache` dictionary for that specific request.

## Self-Healing
Because the Redis connection pool is managed globally, once the Redis service is restored, subsequent requests will automatically resume using the distributed cache without requiring a restart of the Odoo workers.

## Monitoring
Administrators can check the health of the Redis connection at any time through the Distributed Cache View (`COMM_distributed_cache_view`) in the Odoo settings.

## Anchor reference

Full entry for the anchor named above (the status check described under Monitoring).

### `COMM_distributed_cache_view` [@ANCHOR: COMM_distributed_cache_view]

**Purpose.** The administrator's control panel for the cache: a form in which a model can be picked for invalidation and the health of this server's Redis connection checked.

**Definition.** View `view_distributed_cache_config_form` on the transient model `distributed.cache.config` (`views/distributed_cache_views.xml`), opened in a dialog by the action `action_distributed_cache_config`. Fields: `model_id` (Many2one `ir.model`, required, no create) and `name`. Footer buttons: **Invalidate Model Cache** (`action_invalidate_model_cache`, `COMM_manual_cache_invalidation`), **Check Redis Status** (`check_redis_status`) and Cancel. The menu **Settings > Technical > Distributed Cache** (`menu_distributed_cache_root`) is shown to the group `distributed_redis_cache.group_distributed_cache_manager`.

**Behaviour.** Check Redis Status does a `PING` through `get_redis_connection(env)` and shows a green "healthy", a red failure message with the Redis error, or a yellow notice that the local fallback cache is active when Redis is not available. It tests only this Odoo server's connection, not the daemon. The records are transient and hold no cache data; the Redis connection fields live on the Settings page, not here.

**See also.** Tests: `tests/test_distributed_cache.py` and the tour `static/src/js/distributed_cache_tour.js`.
