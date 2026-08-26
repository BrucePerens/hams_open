# Story: Cache Quota Management

## Context
Browsers impose strict storage limits on Service Workers. If a Service Worker exceeds its quota, the browser may purge the entire cache, leading to performance degradation.

## The Problem
Odoo instances can have many modules with numerous static assets (JS, CSS, images). Summing all these can easily exceed the safe limits (~35MB) of some browser environments.

## The Solution
The `caching` module implements a dynamic safety valve.

1. **Scanning**: During server startup or the first request, the system scans all `static/` directories of installed modules ([@ANCHOR: COMM_caching_fs_scan_logic]).

2. **Calculation**: It then calculates a dynamic maximum file size limit ([@ANCHOR: COMM_caching_quota_calculation]).
3. **Filtering**: If the total size of all assets exceeds the `caching.safe_quota_mb` (default 35MB), the system identifies the largest files and excludes them from the cache until the total remaining size fits within the quota.
4. **Enforcement**: This calculated `MAX_FILE_SIZE_BYTES` is injected into the Service Worker script ([@ANCHOR: COMM_caching_sw_serve_route]).

## Impact
This ensures that the most critical, lightweight UI assets (JS/CSS) are always cached, while heavy media files that would risk the entire cache's stability are safely ignored by the Service Worker, allowing standard browser caching or CDNs to handle them.

## Client-Side LRU Bookkeeping
Enforcing the quota client-side (`enforceLRUQuota`, triggered whenever `navigator.storage.estimate()` reports usage past `MAX_STORAGE_BYTES`) requires knowing which cached URL was least recently used, so the Service Worker keeps its own `LRUMetadata` IndexedDB store (one row per cached URL, with a `timestamp` index) alongside the Cache Storage entries themselves. Opening that database (`openDB()`) is expected to settle normally on every real browser ([@ANCHOR: sw_idb_open_db_normal]); a real IndexedDB open failure (private-browsing quota exhaustion, corrupted database, etc.) must make `openDB()` reject cleanly rather than hang the Service Worker forever ([@ANCHOR: sw_idb_open_db_forced_error]).
