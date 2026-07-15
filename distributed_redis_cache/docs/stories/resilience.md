<!-- Copyright © Bruce Perens K6BP. AGPL-3.0. -->

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
Administrators can check the health of the Redis connection at any time through the Distributed Cache View ([@ANCHOR: COMM_distributed_cache_view]) in the Odoo settings.
