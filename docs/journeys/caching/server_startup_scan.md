# Journey: Server Startup Scan

This journey describes the background process that configures the Service Worker's dynamic parameters during Odoo startup.

1. **Trigger**: When the first request for `/sw.js` arrives, or during initial worker boot ([@ANCHOR: caching_sw_serve_route]).
2. **Module Discovery**: The system queries the database for all installed modules.
3. **Filesystem Walk**: For each module, it recursively scans the `static/` directory ([@ANCHOR: caching_fs_scan_logic]).
4. **Metric Collection**:
   - It captures the `mtime` of every file to find the most recent change.
   - It records the size of every file.
5. **Quota Analysis**:
   - The system retrieves the `caching.safe_quota_mb` parameter.
   - It sorts all file sizes in descending order.
   - It iteratively subtracts the largest files until the remaining sum fits within the safe quota ([@ANCHOR: caching_quota_calculation]).
6. **Parameter Finalization**: It determines the final `latest_mtime` and the `dynamic_max_size` (the size of the first file that *didn't* fit).
7. **Injection**: These values are substituted into the `sw.js` template before it is served to the client.
8. **Caching**: The results of the scan are cached in RAM (`_fs_cache`) to avoid re-scanning the disk on subsequent requests.
