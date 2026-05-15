# Story: Cache Invalidation Strategy

## Context
Static assets in Odoo are often cached aggressively. When a developer updates a JS or CSS file, users must receive the update immediately without needing a manual "hard refresh".

## The Problem
Service Workers persist their cache until explicitly told otherwise. Traditional cache-busting (query parameters) is often insufficient for Service Worker management.

## The Solution
The `caching` module uses a filesystem-linked versioning strategy.

1. **Detection**: The system monitors the maximum `mtime` (modification time) of all files in the `static/` directories of all installed modules ([@ANCHOR: caching_fs_scan_logic]).
2. **Injection**: This timestamp is used to generate a unique `CACHE_NAME` (e.g., `odoo-assets-cache-1712345678-v1`) which is injected into the `/sw.js` file whenever it is served ([@ANCHOR: caching_sw_serve_route]).
3. **Activation**: When the Service Worker script is updated (because a file changed and the `mtime` increased), the browser detects the change, installs the new worker, and the `activate` event listener in the SW purges all old caches that don't match the new `CACHE_NAME`.
4. **Manual Override**: Administrators can also increment the `caching.invalidation_version` system parameter to force a global cache purge without changing any files.

## Impact
Users always have the most up-to-date assets within one page load of a server restart or file change, ensuring UI consistency without manual intervention.
