# Caching PWA & Service Worker (`caching`)

*Copyright © Bruce Perens K6BP. Licensed under the GNU Affero General Public License v3.0 or later (AGPL-3.0-or-later).*

This module optimizes the Odoo frontend performance by implementing a client-side CDN (content delivery network: here the browser's own cache storage serves the assets, as a CDN edge server would) via a global Service Worker. It significantly reduces page load times and server load by caching static assets directly in the user's browser.

When a user loads a page, the Service Worker intercepts the requests for Odoo's JavaScript, CSS, and static module files. If the browser already has a copy of the file, it loads it instantly from the browser's cache storage without ever talking to the network. Only successful (HTTP 200) same-origin `GET` responses are stored.

## 🪄 How It Works (Zero-Config)

You do not need to do anything special to make your custom modules work with this cache.

The Service Worker automatically looks for requests matching these patterns:
* `/web/assets/...` (Odoo's compiled JS/CSS bundles)
* `/web/static/...` (Core Odoo static files)
* `/<your_module_name>/static/...` (Your custom module's frontend assets)

As long as you place your Javascript, CSS, and UI icons inside your module's standard `static/` directory, they will be cached automatically.

Some requests are never cached even when they match: `/static/description/images/` (module documentation images), and anything under `/my/`, `/api/`, `/web/image/` or `/web/content/`. Page navigations are not cached either: they always go to the network, and fall back to `/offline` only when the network request fails.

A file that is already cached is served from the cache without asking the server for a newer copy, so a changed file at the same URL is not picked up until the cache name changes (see Automated Cache Invalidation below). Odoo's compiled bundles avoid this because they are versioned by a hash in their URL (`/web/assets/<hash>/<bundle>`); when the worker caches a bundle under a new hash, it deletes the copy cached under the old one.

## 📱 Progressive Web App (PWA) Capabilities

The module turns the website into an installable Progressive Web App (PWA):
- **`/manifest.json`**: Dynamically generates the PWA manifest using the theme and background colors configured in the Website Settings.
- **`/offline`**: A dedicated fallback route, stored in the cache when the Service Worker installs, that is served by the Service Worker when the client loses internet connectivity (if even that cached copy is missing, the worker returns a minimal built-in offline page), providing a graceful offline experience instead of a browser error.

## 🏢 Multi-Tenant & Multi-Website Support

The caching system is fully aware of Odoo's multi-website architecture. Each website can have its own independent caching configuration:
- **Individual Quotas:** Large websites with many assets can have higher quotas, while smaller ones stay lean.
- **Independent Invalidation:** Clicking **Invalidate Cache Now** for one website changes only that website's cache version, so users of other websites don't re-download their cached assets. (A changed static file, by contrast, changes the cache name for every website, because the filesystem scan below covers all modules.)

## 🔄 Automated Cache Invalidation

This module eliminates the need for manual version bumping or complex cache-busting query parameters.

**Filesystem-Linked Invalidation:**
- **Filesystem Scan:** When `/sw.js` is requested and no cached scan result exists (nothing scans during server startup itself), the module performs an efficient recursive scan using `os.scandir` of all `static/` directories across all installed modules (entry `COMM_caching_fs_scan_logic` above). Hidden files (starting with `.`) are ignored to prevent caching metadata or source control files; `static/description/` is skipped and symbolic links are not followed. The result is cached through `distributed_redis_cache`'s `@distributed_cache()`: in each worker's memory, and in Redis for up to 24 hours.
- **MTime Tracking:** It identifies the latest modification timestamp (`mtime`) among all discovered assets.
- **Dynamic Service Worker (SW) Generation:** This timestamp is injected into the `/sw.js` payload as part of the cache name `odoo-assets-cache-<mtime>-v<version>` (the version is the website's invalidation version), effectively versioning the Service Worker script itself. The per-file size limit and the quota are injected the same way.
- **Automatic Refresh:** When a new scan finds a different latest modification time, the Service Worker's signature changes. A server restart alone does not guarantee a new scan: it empties only each worker's in-memory copy of the scan result, and the Redis copy can survive for up to 24 hours. (The `/sw.js` template text itself is also cached in Redis for 24 hours.) Browsers detect this update on the next visit (`/sw.js` is served with `Cache-Control: no-cache`), triggering a background installation of the new worker, which takes over immediately and purges this module's stale `odoo-assets-cache-*` caches; open pages then show an "Update Available" notice with a **Reload** button.

## 🚨 The File-Size Caveat & Safety Valve

Browsers give Service Workers a strict storage limit. If a Service Worker tries to cache massive files, it will max out the quota and the browser will panic and delete the entire cache—destroying the performance benefits of this module.

**The Dynamic Safety Valve:** To protect against this, the server runs a calculation before serving `/sw.js`. It sums up the sizes of all static files found by the scan. If the total size exceeds the safe limits of the browser's cache quota (configurable per website, default 35MB, minus 10MB), it automatically calculates a dynamic max file size limit: it drops the largest files one at a time until the rest fit, and sets the limit just below the size of the last file dropped. It prioritizes small files while rejecting the largest ones to stay under the quota. If everything fits, the limit is the larger of 10MB and the largest file plus 1KB; if the quota is 10MB or less, the limit is 0 and no module static file is cached. The calculation always sets aside 10MB of the quota for critical Odoo bundles (`/web/assets/`) and system overhead; bundles are exempt from the per-file limit.

In the browser, the Service Worker also enforces the quota itself. It records when each non-bundle file was last used (in IndexedDB), and whenever this site's storage use exceeds the quota after caching a file, it deletes the 10 least recently used non-bundle files. If the browser rejects a non-bundle write as over quota, it evicts the same way and retries that write once.

**The Golden Rule:** Keep your `static/` folders strictly reserved for lightweight UI code (JS, CSS) and small layout graphics. If you need to serve heavy media, user uploads, or large datasets, use Odoo's standard attachment routes (`/web/image` or `/web/content`). The Service Worker explicitly ignores those routes, leaving them to the network (and to a CDN such as Cloudflare, if one is in front of the site) to handle the heavy lifting safely.

---

## 🛠 Troubleshooting for Administrators

If you notice that users are seeing old versions of your website after you've updated your custom module's CSS or JS:

1.  **Restart the Odoo Server:** This clears each worker's in-memory scan result so that the next scan can update the Service Worker's internal versioning based on the latest file modification times. When Redis holds a cached scan result (up to 24 hours old), a restart alone may not change anything; step 2 always does.
2.  **Manual Invalidation:** Go to **Website Settings** and click **Invalidate Cache Now**. This will force every browser to purge its local cache and download fresh assets on the next page load. It applies to the website being configured, and it works by changing the cache version, not by rescanning, so it is effective even while the scan result is stale.
3.  **Check for Hidden Files:** Ensure your assets do not start with a dot (`.`), as the scanner intentionally ignores them.
4.  **Check Browser Quota:** If you've added many large files to `static/`, they might be hitting the **Safe Quota**. Increase the quota in settings if your website has a large amount of essential frontend code.

# Technical Documentation

**Context:** Technical documentation strictly for LLMs (large language models) and Integrators.

## 1. Overview
Implements a global, root-scoped Service Worker (`/sw.js`) that proxies and caches frontend assets locally in the browser to provide near-instant load times.

## 2. Integration Rules
* Assets placed in your module's `static/` directory are cached automatically.
* **No Competing Workers:** DO NOT attempt to register another Service Worker.
* **WebSockets:** `ws://` and `wss://` protocols are hardcoded to bypass the proxy.
* **Dynamic Large File Prohibition**: The server calculates a per-file size limit from the website's quota (default 35MB) and injects it into the worker. Heavy media MUST route via `/web/image` to prevent the cache from ejecting critical UI bundles.

* **Settings Layout Injection**: The settings UI is injected into `website.res_config_settings_view_form` via XPath (entry `COMM_xpath_rendering_caching_settings` below).

## 3. Zero-Sudo Architecture
This module is built with security as a primary concern, adhering strictly to the Zero-Sudo architecture:
- **Micro-Privileged Service Account**: A dedicated service user `caching.user_caching_service` is utilized for the filesystem scan (entry `COMM_caching_fs_scan_logic` above). This account has zero access to business data: its only access rights are read access to module records (`ir.module.module`) and to system parameters whose key begins with `caching.`.
- **Secure Parameter Access**: System parameters (in this module, only `caching.enable_sw_test_hooks`, which turns on the Service Worker's test-only message hooks and is off unless set to `true` or `1`) are retrieved through the `zero_sudo.security.utils` abstraction layer, preventing direct access to `ir.config_parameter` and maintaining strict audit trails.
- **Configuration Whitelisting**: Only specifically approved parameters are readable through that layer, preventing unauthorized configuration leakage. The one this module reads, `caching.enable_sw_test_hooks`, is on `zero_sudo`'s whitelist. The quota and invalidation version are not system parameters at all: they are per-website fields (`website.caching_safe_quota_mb`, `website.caching_invalidation_version`). (`caching.safe_quota_mb` is also whitelisted, but nothing in this module reads it.)
- **No Sudo Escalation**: All background operations run within the context of their assigned service accounts without ever requesting global administrative (`sudo`) privileges.

### Service worker details
- **Offline fallback page** [@ANCHOR: caching_sw_offline_fallback_response]: when a page request fails and even the precached `/offline` page cannot be found (evicted, or its install-time precache failed), the worker answers with a small built-in "You are offline" page, status 503, instead of returning nothing, which the browser would show as a hard network error.
- **Stale cache cleanup** [@ANCHOR: caching_sw_cache_cleanup_prefix_scoped]: on activation the worker deletes the caches left by older versions of itself, and only those: a cache is deleted only if its name starts with this worker's own prefix and is not the current cache's name, so caches that other scripts on the same origin created are never touched.

## Anchored functions and views

### `caching.mixin.get_fs_stats(override_svc_uid=None)` [@ANCHOR: COMM_caching_fs_scan_logic]

Scans the `static/` directory of every installed module (`odoo.modules.module.get_modules()` and `get_module_path`) recursively with `os.scandir` and returns `(latest_mtime, file_sizes)`: `latest_mtime` is the newest `st_mtime` as a float (0.0 when there are no files), `file_sizes` is a list of every file's size in bytes sorted largest first. Dot-named entries and a module's `static/description` folder are skipped and symlinks are not followed. An unreadable directory is logged at warning level and skipped, so the scan never raises for a filesystem error. The method is an `@api.model`, `@api.private` method wrapped in `@distributed_cache()`, so the result is shared through Redis and may survive a restart for up to 24 hours; `force_invalidate_cache` clears it.

| Argument | Type | Meaning |
|---|---|---|
| `override_svc_uid` | int or None | The user id of `caching.user_caching_service`. When given, the call really switches to that user with `with_user`, so any ORM read added later runs with the service account's narrow rights (read on `ir.module.module` and on `caching.` parameters) rather than the public visitor's. |

Security: the scan reads only file names, times and sizes under installed modules' `static/` folders; it does not read file contents or business data. Tests: `tests/test_settings_and_cache.py` (`test_05_zero_sudo_scan`, anchor `test_caching_zero_sudo_scan`).

### `caching.mixin.get_global_static_info(quota_mb, override_svc_uid=None)` [@ANCHOR: COMM_caching_quota_calculation]

Works out the largest single file the service worker may cache. It calls `get_fs_stats`, takes a safe quota of `max(0, quota_mb - 10)` MB (10 MB is reserved for Odoo's own bundles, which the worker exempts) and returns `(str(int(latest_mtime)), str(max_file_size))`, both strings because they are pasted into JavaScript. If the safe quota is zero or less the limit is `"0"`; with no files it is 10 MB; when everything fits, the limit is the larger of the biggest file plus 1 KiB and 10 MB; otherwise it walks the sorted sizes from the largest, dropping files until the rest fit, and the limit is just below the size of the last file dropped (so that file and all larger ones are excluded). Wrapped in `@distributed_cache()` like `get_fs_stats`. Argument `quota_mb` is the website's `caching_safe_quota_mb` (default 35); `override_svc_uid` as above. No errors of its own. Test: `tests/test_settings_and_cache.py` (anchor `test_settings_and_cache_01`).

### `ServiceWorkerController.service_worker()`, route `/sw.js` [@ANCHOR: COMM_caching_sw_serve_route]

Serves the service worker from the site root, which is the only way it can control the whole origin. `type="http"`, `auth="public"`, `website=True`, no sitemap entry; it takes no arguments and answers `200` with `Content-Type: application/javascript` and `Cache-Control: no-cache, max-age=0`. Steps: read the template `caching/static/src/sw/sw.js` (cached as the Redis key `caching_sw_js_content` for 24 hours; if Redis fails the read and write are logged at warning level and the file is read from disk, never an HTTP 500); take the current website's `caching_safe_quota_mb` and `caching_invalidation_version` (35 and 1 when there is no website); when the context has `force_fs_scan` call `force_invalidate_cache` as the service account; call `get_global_static_info`; and replace the placeholders `__CACHE_NAME__` (`odoo-assets-cache-<mtime>-v<version>`), `__MAX_FILE_SIZE_BYTES__`, `__MAX_STORAGE_BYTES__` (quota in bytes) and `__TEST_HOOKS_ENABLED__` (`true` only when the whitelisted parameter `caching.enable_sw_test_hooks` is `1` or `true`). Raises `NotFound` (404) when the template file is missing. Runs for anonymous visitors, uses the service account for the scan and sudo nowhere. Tests: `tests/test_service_worker.py`.

### Service worker `fetch` listener [@ANCHOR: COMM_caching_sw_fetch_interceptor]

The `fetch` event handler in `static/src/sw/sw.js`; it decides, per request, whether the worker answers from its cache. It ignores (lets the browser handle) anything that is not `GET`, the Chrome `only-if-cached` cross-origin case, `ws:`/`wss:` URLs, `/my/`, `/api/`, `/web/image/`, `/web/content/` and documentation images. A page navigation is network-first and falls back to the precached `/offline` page, then to a built-in 503 page. A request whose path matches `CACHE_URL_REGEX` (`/web/assets/` or `/<module>/static/`) is answered from `CACHE_NAME` when present (recording its use for the LRU unless it is an Odoo bundle); otherwise it is fetched, and only a status-200 `basic` response is cached. A non-bundle response over `MAX_FILE_SIZE_BYTES` (by `Content-Length`, or measured when that header is absent) is returned but not stored. The cache write is not awaited and its errors are only logged; a `QuotaExceededError` evicts the ten oldest entries and retries once; a newly cached Odoo bundle deletes the older hash of the same bundle. Tests: `tests/test_sw_fetch.py` and the tours `sw_behavior_tour.js` and `caching_tour.js`.

### `res.config.settings` view `res_config_settings_view_form` [@ANCHOR: COMM_xpath_rendering_caching_settings]

`views/res_config_settings_views.xml` inherits `website.res_config_settings_view_form` and, by an XPath after the `website_settings` block, adds a "Caching Service Worker" block with four settings: Safe Quota (`caching_safe_quota_mb`, integer MB), Force Cache Invalidation (shows `caching_invalidation_version` and the button `action_force_cache_invalidation`, "Invalidate Cache Now"), PWA Theme Color (`caching_pwa_theme_color`) and PWA Background Color (`caching_pwa_background_color`). All four are per-website fields. Pressing the button raises the version by one, which changes the cache name served by `/sw.js` and so makes every browser of that website drop its old caches. No arguments; the view is data, not code, and is loaded at install. Test: `tests/test_settings_and_cache.py` (`test_04_xpath_rendering_settings`).

### Test `test_03_caching_sudo_params` [@ANCHOR: COMM_test_caching_sudo_params]

A test in `tests/test_settings_and_cache.py` that sets `caching.safe_quota_mb` to `"42"` through `zero_sudo.security.utils._set_system_param` and reads it back with `_get_system_param`, asserting `"42"`. It shows that a whitelisted `caching.` parameter is reachable through the Zero-Sudo layer without a direct `ir.config_parameter` or `sudo()` call. It takes no arguments, changes only the test database (rolled back with the test) and fails with an assertion error if the value does not round trip. It also stands as the anchor named by the Manual Invalidation journey.

## 4. Stories & Journeys
Detailed architectural narratives and process flows are documented in the `docs/` directory:

### Stories
* [Cache Quota Management](docs/stories/cache_quota_management.md) (entry `COMM_caching_quota_calculation` above)

* [Cache Invalidation Strategy](docs/stories/cache_invalidation_strategy.md) (entry `COMM_caching_fs_scan_logic` above)

### Journeys
* [Asset Request Flow](docs/journeys/asset_request_flow.md) (entry `COMM_caching_sw_fetch_interceptor` above)

* [Server Startup Scan](docs/journeys/server_startup_scan.md) (entry `COMM_caching_sw_serve_route` above)

* [Manual Invalidation](docs/journeys/manual_invalidation.md) (entry `COMM_test_caching_sudo_params` above)

## 5. Testing
Tests are located in the `tests/` directory and cover:
- Service Worker delivery and headers.

- Quota calculation logic.
- Cache invalidation triggers.
- UI Tour for registration check.

- Zero-Sudo compliance for FS scan.

## 6. External Dependencies
None
