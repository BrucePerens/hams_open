# Story: Cache Quota Management

## Context
Browsers impose strict storage limits on Service Workers. If a Service Worker exceeds its quota, the browser may purge the entire cache, leading to performance degradation.

## The Problem
Odoo instances can have many modules with numerous static assets (JS, CSS, images). Summing all these can easily exceed the safe limits (~35MB) of some browser environments.

## The Solution
The `caching` module implements a dynamic safety valve.

1. **Scanning**: During server startup or the first request, the system scans all `static/` directories of installed modules ([@ANCHOR: caching_fs_scan_logic]).
2. **Calculation**: It then calculates a dynamic maximum file size limit ([@ANCHOR: caching_quota_calculation]).
3. **Filtering**: If the total size of all assets exceeds the `caching.safe_quota_mb` (default 35MB), the system identifies the largest files and excludes them from the cache until the total remaining size fits within the quota.
4. **Enforcement**: This calculated `MAX_FILE_SIZE_BYTES` is injected into the Service Worker script ([@ANCHOR: caching_sw_serve_route]).

## Impact
This ensures that the most critical, lightweight UI assets (JS/CSS) are always cached, while heavy media files that would risk the entire cache's stability are safely ignored by the Service Worker, allowing standard browser caching or CDNs to handle them.
