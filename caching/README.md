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
- **Filesystem Scan:** When `/sw.js` is requested and no cached scan result exists (nothing scans during server startup itself), the module performs an efficient recursive scan using `os.scandir` of all `static/` directories across all installed modules ([@ANCHOR: COMM_caching_fs_scan_logic]). Hidden files (starting with `.`) are ignored to prevent caching metadata or source control files; `static/description/` is skipped and symbolic links are not followed. The result is cached through `distributed_redis_cache`'s `@distributed_cache()`: in each worker's memory, and in Redis for up to 24 hours.
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
* **Dynamic Large File Prohibition**: The server calculates a per-file size limit from the website's quota (default 35MB) and injects it into the worker [@ANCHOR: COMM_caching_quota_calculation]. Heavy media MUST route via `/web/image` to prevent the cache from ejecting critical UI bundles.

* **Settings Layout Injection**: The settings UI is injected into `website.res_config_settings_view_form` via XPath [@ANCHOR: COMM_xpath_rendering_caching_settings].

## 3. Zero-Sudo Architecture
This module is built with security as a primary concern, adhering strictly to the Zero-Sudo architecture:
- **Micro-Privileged Service Account**: A dedicated service user `caching.user_caching_service` is utilized for the filesystem scan ([@ANCHOR: COMM_caching_fs_scan_logic]). This account has zero access to business data: its only access rights are read access to module records (`ir.module.module`) and to system parameters whose key begins with `caching.`.
- **Secure Parameter Access**: System parameters (in this module, only `caching.enable_sw_test_hooks`, which turns on the Service Worker's test-only message hooks and is off unless set to `true` or `1`) are retrieved through the `zero_sudo.security.utils` abstraction layer, preventing direct access to `ir.config_parameter` and maintaining strict audit trails.
- **Configuration Whitelisting**: Only specifically approved parameters are readable through that layer, preventing unauthorized configuration leakage. The one this module reads, `caching.enable_sw_test_hooks`, is on `zero_sudo`'s whitelist. The quota and invalidation version are not system parameters at all: they are per-website fields (`website.caching_safe_quota_mb`, `website.caching_invalidation_version`). (`caching.safe_quota_mb` is also whitelisted, but nothing in this module reads it.)
- **No Sudo Escalation**: All background operations run within the context of their assigned service accounts without ever requesting global administrative (`sudo`) privileges.

## 4. Stories & Journeys
Detailed architectural narratives and process flows are documented in the `docs/` directory:

### Stories
* [Cache Quota Management](docs/stories/cache_quota_management.md) ([@ANCHOR: COMM_caching_quota_calculation])

* [Cache Invalidation Strategy](docs/stories/cache_invalidation_strategy.md) ([@ANCHOR: COMM_caching_fs_scan_logic])

### Journeys
* [Asset Request Flow](docs/journeys/asset_request_flow.md) ([@ANCHOR: COMM_caching_sw_fetch_interceptor])

* [Server Startup Scan](docs/journeys/server_startup_scan.md) ([@ANCHOR: COMM_caching_sw_serve_route])

* [Manual Invalidation](docs/journeys/manual_invalidation.md) ([@ANCHOR: COMM_test_caching_sudo_params])

## 5. Testing
Tests are located in the `tests/` directory and cover:
- Service Worker delivery and headers [@ANCHOR: COMM_caching_sw_serve_route].

- Quota calculation logic [@ANCHOR: COMM_caching_quota_calculation].
- Cache invalidation triggers.
- UI Tour for registration check [@ANCHOR: COMM_caching_sw_fetch_interceptor].

- Zero-Sudo compliance for FS scan [@ANCHOR: COMM_caching_fs_scan_logic].

## 6. External Dependencies
None
