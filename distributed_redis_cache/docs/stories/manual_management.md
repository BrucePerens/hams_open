<!-- Copyright © Bruce Perens K6BP. AGPL-3.0. -->

# Story: Manual Cache Management

While the cache is designed to be fully automated, administrators occasionally need manual control over the invalidation process.

## The Management Interface
The module provides a dedicated UI ([@ANCHOR: COMM_distributed_cache_view]) where administrators can:
- **Monitor Status**: Verify that Odoo is successfully communicating with the Redis backend.
- **Targeted Invalidation**: Select a specific Odoo model and trigger a manual cache flush ([@ANCHOR: COMM_manual_cache_invalidation]).

## Redis Configuration
The Redis connection settings can be configured via the **Settings** menu under the **Distributed Redis Cache** section ([@ANCHOR: COMM_distributed_cache_settings_view]).

## Safety First
Manual invalidation still follows the standard invalidation pipeline, ensuring that the cache is cleared across the *entire* cluster, not just on the administrator's current worker.

## Automatic Documentation
Upon installation, the module automatically injects its comprehensive documentation into the Odoo Knowledge base or Knowledge [@ANCHOR: COMM_doc_inject_distributed_redis_cache], ensuring that administrators have immediate access to these instructions.
