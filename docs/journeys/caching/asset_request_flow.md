# Journey: Asset Request Flow

This journey describes how a request for a frontend asset is handled by the Service Worker.

1. **Interception**: The browser initiates a `GET` request for an asset (e.g., `/web/assets/debug/web.assets_backend.js`).
2. **SW Catch**: The Service Worker's `fetch` event listener intercepts the request ([@ANCHOR: caching_sw_fetch_interceptor]).
3. **Regex Match**: The SW checks the URL against `CACHE_URL_REGEX` to see if it belongs to Odoo's core assets or a module's `static/` directory.
4. **Cache Lookup**:
   - **Hit**: If the asset is found in `CACHE_NAME`, it is returned immediately (0ms latency).
   - **Miss**: If not found, the SW proceeds to the next step.
5. **Network Fetch**: The SW fetches the asset from the network.
6. **Safety Check**: Before caching the network response, the SW checks the `Content-Length`.
   - If the size exceeds `MAX_FILE_SIZE_BYTES`, it is returned to the browser but **not** cached to protect the quota.
7. **Cache Update**: If the size is within limits, the response is cloned and stored in the cache for future requests.
8. **Completion**: The asset is delivered to the browser.
