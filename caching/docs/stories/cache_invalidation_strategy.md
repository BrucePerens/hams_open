# Story: Cache Invalidation Strategy

## Context
Static assets in Odoo are often cached aggressively. When a developer updates a JS or CSS file, users must receive the update immediately without needing a manual "hard refresh".

## The Problem
Service Workers persist their cache until explicitly told otherwise. Traditional cache-busting (query parameters) is often insufficient for Service Worker management.

## The Solution
The `caching` module uses a filesystem-linked versioning strategy.

1. **Detection**: The system monitors the maximum `mtime` (modification time) of all files in the `static/` directories of all installed modules ([@ANCHOR: COMM_caching_fs_scan_logic]). `caching.mixin.get_fs_stats` walks each installed module's `static/` directory recursively with `os.scandir`, skipping dot-files and each module's `static/description` folder, never following symlinks, and returns the newest `st_mtime` it saw together with every file size sorted largest first (the sizes feed the quota story). It runs as the narrow `caching.user_caching_service` account and its result is kept by `@distributed_cache`, so a server restart alone may not trigger a new scan.

2. **Injection**: This timestamp is used to generate a unique `CACHE_NAME` (e.g., `odoo-assets-cache-1712345678-v1`) which is injected into the `/sw.js` file whenever it is served ([@ANCHOR: COMM_caching_sw_serve_route]).
3. **Activation**: When the Service Worker script is updated (because a file changed and the `mtime` increased), the browser detects the change, installs the new worker, and the `activate` event listener in the SW purges all old caches that don't match the new `CACHE_NAME`.
4. **Manual Override**: Administrators can also increment the `caching.invalidation_version` system parameter to force a global cache purge without changing any files. (Per the [module README](../../README.md), the version is a per-website field, `website.caching_invalidation_version`, changed with **Invalidate Cache Now**, and it purges only that website's caches.)

## Impact
Users always have the most up-to-date assets within one page load of a file change (a server restart alone does not guarantee a new scan, because the scan result can survive in Redis for up to 24 hours; see the README's Automated Cache Invalidation), ensuring UI consistency without manual intervention.
