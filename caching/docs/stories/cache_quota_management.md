# Story: Cache Quota Management

## Context
Browsers impose strict storage limits on Service Workers. If a Service Worker exceeds its quota, the browser may purge the entire cache, leading to performance degradation.

## The Problem
Odoo instances can have many modules with numerous static assets (JS, CSS, images). Summing all these can easily exceed the safe limits (~35MB) of some browser environments.

## The Solution
The `caching` module implements a dynamic safety valve.

1. **Scanning** (the [module README](../../README.md) states the scan runs when `/sw.js` is first requested without a cached result, not at server startup, and that the quota is the per-website field `website.caching_safe_quota_mb`, not the system parameter named in step 3): During server startup or the first request, the system scans all `static/` directories of installed modules ([@ANCHOR: COMM_caching_fs_scan_logic]).

2. **Calculation**: It then calculates a dynamic maximum file size limit ([@ANCHOR: COMM_caching_quota_calculation]): `caching.mixin.get_global_static_info(quota_mb)` subtracts a 10 MB reserve for Odoo's own bundles from the website's quota, and returns the newest mtime and the largest file size that may still be cached, both as strings.
3. **Filtering**: If the total size of all assets exceeds the `caching.safe_quota_mb` (default 35MB), the system identifies the largest files and excludes them from the cache until the total remaining size fits within the quota.
4. **Enforcement**: This calculated `MAX_FILE_SIZE_BYTES` is injected into the Service Worker script ([@ANCHOR: COMM_caching_sw_serve_route]) by the `/sw.js` route, which replaces the `__CACHE_NAME__`, `__MAX_FILE_SIZE_BYTES__`, `__MAX_STORAGE_BYTES__` and `__TEST_HOOKS_ENABLED__` placeholders in `static/src/sw/sw.js` before sending it. The worker then refuses to cache a non-bundle response larger than that limit.

## Impact
This ensures that the most critical, lightweight UI assets (JS/CSS) are always cached, while heavy media files that would risk the entire cache's stability are safely ignored by the Service Worker, allowing standard browser caching or CDNs to handle them.

## Client-Side LRU Bookkeeping
Enforcing the quota client-side (`enforceLRUQuota`, triggered whenever `navigator.storage.estimate()` reports usage past `MAX_STORAGE_BYTES`) requires knowing which cached URL was least recently used, so the Service Worker keeps its own `LRUMetadata` IndexedDB store (one row per cached URL, with a `timestamp` index) alongside the Cache Storage entries themselves. Opening that database (`openDB()`) is expected to settle normally on every real browser ([@ANCHOR: sw_idb_open_db_normal]); a real IndexedDB open failure (private-browsing quota exhaustion, corrupted database, etc.) must make `openDB()` reject cleanly rather than hang the Service Worker forever ([@ANCHOR: sw_idb_open_db_forced_error]).

## Service worker functions, each tested by `tests/test_sw_unit.py`

All four live in `static/src/sw/sw.js`, run inside the browser's Service Worker (a separate global scope from the page, with its own `indexedDB`), and touch no server, database or transmitter. The bookkeeping database is `LRUCacheDB` version 1, with one object store `LRUMetadata` (key path `url`, index `timestamp` not unique); each row is `{url: string, timestamp: milliseconds since the epoch}`.

### `openDB()` [@ANCHOR: caching:js_sw_open_db]

`function openDB(): Promise<IDBDatabase>`. Opens `LRUCacheDB` at version 1. On the first open (`onupgradeneeded`) it creates the `LRUMetadata` store with keyPath `url` and the non-unique `timestamp` index; if the store already exists nothing is created. Resolves with the open database handle. Rejects with the request's own error when the browser refuses to open the database (private-browsing quota, corruption), so callers never hang; when the test-only flag `__testForceIdbError` is set (settable only through the `TEST_FORCE_IDB_ERROR` message hook, honoured only if the served script was built with test hooks on) it rejects at once with `TEST_FORCED_IDB_ERROR`. Asynchronous: the result arrives on a later event-loop turn. Callers: `updateLRUMetadata` and `enforceLRUQuota`. Tests: `test_open_db_creates_the_store_once_and_rejects_on_error` (harness body `open_db`).

### `updateLRUMetadata(url)` [@ANCHOR: caching:js_sw_update_lru_metadata]

`async function updateLRUMetadata(url: string): Promise<void>`. Records that the cached URL `url` was just used: it opens the database, starts a readwrite transaction and `put`s `{url, timestamp: Date.now()}`, replacing any earlier row for the same URL. The promise resolves when the transaction completes and rejects if the transaction errors or aborts. A failure to open the database is caught, logged with `console.error` ("IDB update error") and swallowed, so the promise then resolves with `undefined`; caching must never fail a page request. The fetch handler calls it for non-bundle assets (Odoo bundles under `/web/assets/` are exempt from LRU tracking) and often does not await it. Tests: `test_update_lru_metadata_records_the_url_and_time`.

### `enforceLRUQuota(cache)` [@ANCHOR: caching:js_sw_enforce_lru_quota]

`async function enforceLRUQuota(cache: Cache): Promise<void>`. Keeps the origin's storage under the quota. `cache` is the open `CACHE_NAME` Cache Storage cache whose entries may be deleted. It returns at once, doing nothing, when `navigator.storage.estimate` does not exist or reports `usage` at or below `MAX_STORAGE_BYTES` (the website's quota in bytes, substituted by the server). Over the quota it opens the database and walks the `timestamp` index from the oldest row, and for the first ten rows deletes the URL from `cache` (a failed cache delete is only logged) and deletes the row; with fewer than ten rows all go. Ten at a time is deliberate: a batch frees space quickly without a storage estimate per entry. Resolves when the cursor is exhausted or ten are done; rejects on a cursor or transaction error; an error opening the database is logged ("IDB quota enforcement error") and swallowed. It is called after a successful cache write, and once more after a `QuotaExceededError` from `cache.put` (then the put is retried a single time). Tests: `test_enforce_lru_quota_evicts_the_ten_oldest_over_quota`.

### `previousVersionCacheExists()` [@ANCHOR: caching:js_sw_previous_version_cache_exists]

`function previousVersionCacheExists(): Promise<boolean>`. Resolves true only when Cache Storage holds a cache whose name starts with this worker's own prefix `odoo-assets-cache-` and is not the current `CACHE_NAME`, that is, an older version of this worker left its cache behind. A cache belonging to another script on the same origin (for example the shack worker's `ham-shack-offline-*`) never counts. The `activate` handler calls it before cleaning stale caches, and posts `{type: 'NEW_VERSION_INSTALLED'}` to open pages only when it is true, so a first-time visitor never sees the "Update Available" toast (see `js_toast.md`). No arguments, read-only, no side effects. Tests: `test_previous_version_cache_exists_only_for_an_older_cache_of_its_own`.

## Test harness functions in `tests/sw_unit_harness.js`, run by `tests/test_sw_unit.py`

The harness is a Node script, `node sw_unit_harness.js <path to sw.js> <test name>`; it exits 0 when the named test passes, 1 on a failed assertion or error, and 2 for an unknown name. It reads the worker source, fills the server placeholders with fixed test values (`odoo-assets-cache-new`, 1000-byte file limit, 5000-byte storage quota, hooks off) and runs it in a `vm` context. It needs only Node's standard library, uses no network and changes nothing outside the process.

### `load(globals)` [@ANCHOR: caching:js_sw_harness_load]

`function load(globals = {}): {context, listeners, self, logged}`. Creates a fresh vm context containing a stand-in `self` (its `addEventListener` records the handler by event name in `listeners`; `clients` starts empty), a `console` whose `error` pushes its arguments to `logged` (log and warn are dropped), a few real globals (`Date`, `Promise`, `Response`, `URL`, `setTimeout`, `queueMicrotask`) and whatever `globals` adds (`indexedDB`, `caches`, `navigator`), then runs the worker source in it. Returns the context (the worker's functions are its properties), the registered listeners, the stand-in `self` and the logged errors. A fresh context per call means tests cannot share state. Used by every test body.

### `fakeIndexedDB(options)` [@ANCHOR: caching:js_sw_harness_fakeIndexedDB]

`function fakeIndexedDB({stores = [], fail = false, transaction} = {}): {indexedDB, seen, db}`. Builds an `indexedDB` stand-in. `stores` names object stores that already exist (an empty list means a new database, so `onupgradeneeded` fires before `onsuccess`); `fail` makes `open()` answer with an error `open failed`; `transaction` is the function the fake `db.transaction` runs. `open(name, version)` records `[name, version]` in `seen.opens` and answers on a `setTimeout(0)` turn, like a real asynchronous request; `createObjectStore` and `createIndex` calls are recorded in `seen.created`. Returns the fake, the record of what happened and the db object. Pure in-memory.

### `plain(value)` [@ANCHOR: caching:js_sw_harness_plain]

`const plain = (value) => JSON.parse(JSON.stringify(value))`. Objects built inside the vm context have that realm's prototypes, which `assert.deepStrictEqual` treats as different from this realm's; the round trip through JSON copies the value into this realm. Argument: any JSON-serialisable value; returns the copy. Functions, `undefined` and cycles are not preserved (a cycle throws), which is fine for the plain records the tests compare.

### `rejection(promise)` [@ANCHOR: caching:js_sw_harness_rejection]

`async function rejection(promise): Promise<Error|null>`. Awaits `promise` and returns null if it resolved, or the value it rejected with. It lets a test assert on a rejection without a try/catch in every test, for example the `open failed` error of `openDB`. Never throws itself.

### Test bodies [@ANCHOR: caching:js_sw_harness_open_db] [@ANCHOR: caching:js_sw_harness_update_lru_metadata] [@ANCHOR: caching:js_sw_harness_enforce_lru_quota] [@ANCHOR: caching:js_sw_harness_previous_version_cache_exists]

The four members of the `tests` table, each an async function with no arguments that throws on a failed assertion and is picked by the third command-line argument. `open_db` checks that a new database gets the `LRUMetadata` store (key path `url`) and the `timestamp` index and is opened as `["LRUCacheDB", 1]`, that an existing store is not recreated, and that an open failure rejects with `open failed`. `update_lru_metadata` checks the single `put` of `{url, timestamp}` with a timestamp between before and after the call, and that an unopenable database is logged once and the call resolves `undefined`. `enforce_lru_quota` checks that no storage API or usage within the quota never opens the database, that over quota the ten oldest of twelve rows are deleted from both the cache and the store, that fewer than ten all go, and that a broken database is logged, not thrown. `previous_version_cache_exists` checks the four cache-name cases (none, only the current, only another script's, an older one of ours) and that the real `activate` handler posts `NEW_VERSION_INSTALLED` only when an old cache existed. Each is started by one Python test in `tests/test_sw_unit.py`, named in that file's `Tests` comments.
