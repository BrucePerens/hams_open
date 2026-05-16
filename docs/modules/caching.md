# ⚡ Caching Module (`caching`)

*Copyright © Bruce Perens K6BP. AGPL-3.0.*

**Context:** Technical documentation strictly for LLMs and Integrators.

## 1. Overview
Implements a global, root-scoped Service Worker (`/sw.js`) that proxies and caches frontend assets locally in the browser to provide near-instant load times.

## 2. Integration Rules
* Assets placed in your module's `static/` directory are cached automatically.
* **No Competing Workers:** DO NOT attempt to register another Service Worker.
* **WebSockets:** `ws://` protocols are hardcoded to bypass the proxy.
* **Dynamic Large File Prohibition:** The worker mathematically calculates an active quota limit (approx 35MB). Heavy media MUST route via `/web/image` to prevent the cache from ejecting critical UI bundles.
* **Layout Injection:** The service worker registration script is injected globally into the frontend `website.layout` via XPath `[@ANCHOR: xpath_rendering_caching_layout]`.

* **Settings Layout Injection**: The settings UI is injected into `website.layout` via XPath `[@ANCHOR: xpath_rendering_caching_settings]`.

## 3. Stories & Journeys
Detailed architectural narratives and process flows are documented in the `docs/` directory:

### Stories
* [Cache Quota Management](caching/docs/stories/cache_quota_management.md) ([@ANCHOR: caching_quota_calculation])
* [Cache Invalidation Strategy](caching/docs/stories/cache_invalidation_strategy.md) ([@ANCHOR: caching_fs_scan_logic])
* [Documentation Bootstrap](caching/docs/stories/documentation_bootstrap.md) ([@ANCHOR: caching_docs_bootstrap])

### Journeys
* [Asset Request Flow](caching/docs/journeys/asset_request_flow.md) ([@ANCHOR: caching_sw_fetch_interceptor])
* [Server Startup Scan](caching/docs/journeys/server_startup_scan.md) ([@ANCHOR: caching_sw_serve_route])
