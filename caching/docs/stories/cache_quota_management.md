# Story: Cache Quota Management

## Context
Browsers impose strict storage limits on Service Workers. If a Service Worker exceeds its quota, the browser may purge the entire cache, leading to performance degradation.

## The Problem
Odoo instances can have many modules with numerous static assets (JS, CSS, images). Summing all these can easily exceed the safe limits (~35MB) of some browser environments.

## The Solution
The `caching` module implements a dynamic safety valve.

1. **Scanning** (the [module README](../../README.md) states the scan runs when `/sw.js` is first requested without a cached result, not at server startup, and that the quota is the per-website field `website.caching_safe_quota_mb`, not the system parameter named in step 3): During server startup or the first request, the system scans all `static/` directories of installed modules ([@ANCHOR: COMM_caching_fs_scan_logic]).

2. **Calculation**: It then calculates a dynamic maximum file size limit ([@ANCHOR: COMM_caching_quota_calculation]).
3. **Filtering**: If the total size of all assets exceeds the `caching.safe_quota_mb` (default 35MB), the system identifies the largest files and excludes them from the cache until the total remaining size fits within the quota.
4. **Enforcement**: This calculated `MAX_FILE_SIZE_BYTES` is injected into the Service Worker script ([@ANCHOR: COMM_caching_sw_serve_route]).

## Impact
This ensures that the most critical, lightweight UI assets (JS/CSS) are always cached, while heavy media files that would risk the entire cache's stability are safely ignored by the Service Worker, allowing standard browser caching or CDNs to handle them.

## Client-Side LRU Bookkeeping
Enforcing the quota client-side (`enforceLRUQuota`, triggered whenever `navigator.storage.estimate()` reports usage past `MAX_STORAGE_BYTES`) requires knowing which cached URL was least recently used, so the Service Worker keeps its own `LRUMetadata` IndexedDB store (one row per cached URL, with a `timestamp` index) alongside the Cache Storage entries themselves. Opening that database (`openDB()`) is expected to settle normally on every real browser ([@ANCHOR: sw_idb_open_db_normal]); a real IndexedDB open failure (private-browsing quota exhaustion, corrupted database, etc.) must make `openDB()` reject cleanly rather than hang the Service Worker forever ([@ANCHOR: sw_idb_open_db_forced_error]).

## Service worker functions, each tested by `tests/test_sw_unit.py`
- `openDB()` opens the `LRUCacheDB` database, creating the `LRUMetadata` store with its timestamp index the first time, and rejects when the browser cannot open it. [@ANCHOR: caching:js_sw_open_db]
- `updateLRUMetadata()` records a cached URL and the time it was last used, and only logs when the database cannot be opened. [@ANCHOR: caching:js_sw_update_lru_metadata]
- `enforceLRUQuota()` does nothing while storage use is within the quota, and over it deletes the ten least recently used entries from the cache and from the bookkeeping. [@ANCHOR: caching:js_sw_enforce_lru_quota]
- `previousVersionCacheExists()` is true only when an older version of this worker left a cache of its own behind (never for another worker's cache), so the "Update Available" toast is not shown on a first visit. [@ANCHOR: caching:js_sw_previous_version_cache_exists]

## Test harness functions in `tests/sw_unit_harness.js`, run by `tests/test_sw_unit.py`
- `load(globals)` runs the worker source in a fresh vm context with stand-ins for the worker globals and returns the context, the listeners the worker registered and what it logged. [@ANCHOR: caching:js_sw_harness_load]
- `fakeIndexedDB(options)` is an `indexedDB` whose `open()` answers asynchronously, creating the store on first open or failing on request. [@ANCHOR: caching:js_sw_harness_fakeIndexedDB]
- `plain(value)` copies an object made inside the vm into this realm so `deepStrictEqual` can compare it. [@ANCHOR: caching:js_sw_harness_plain]
- `rejection(promise)` returns the error a promise rejects with, or null when it resolves. [@ANCHOR: caching:js_sw_harness_rejection]
- `open_db`, `update_lru_metadata`, `enforce_lru_quota` and `previous_version_cache_exists` are the named test bodies the Python tests start, one per worker function. [@ANCHOR: caching:js_sw_harness_open_db] [@ANCHOR: caching:js_sw_harness_update_lru_metadata] [@ANCHOR: caching:js_sw_harness_enforce_lru_quota] [@ANCHOR: caching:js_sw_harness_previous_version_cache_exists]
