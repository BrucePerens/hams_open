<!-- Copyright © Bruce Perens K6BP. AGPL-3.0. -->

# Story: Distributed Cache Decoration

The `distributed_redis_cache` module provides a specialized decorator to ensure cache coherence across multiple Odoo workers.

## The Problem
Standard Odoo caching (`@tools.ormcache`) stores data in the local memory of a single WSGI worker. In a multi-worker or multi-node environment, when data changes, one worker might clear its cache while others continue to serve stale data.

## The Solution
By using the `@distributed_cache()` decorator ([@ANCHOR: COMM_distributed_cache_decorator]), developers can ensure that cached data is stored in a central Redis instance ([@ANCHOR: COMM_redis_connection_pool]).

## Key Features
- **Key Generation**: Cache keys ([@ANCHOR: COMM_distributed_cache_key_generation]) are automatically generated based on serialized function arguments using SHA256, ensuring unique and collision-resistant storage.
- **Fail-Open Resilience**: If Redis becomes unavailable, the system automatically falls back to a local memory cache, ensuring the application remains functional even if the cache cluster is down.
- **Testing Safety**: The decorator automatically disables Redis interaction during Odoo tests to prevent "ghost" cache poisoning from rolled-back transactions.
