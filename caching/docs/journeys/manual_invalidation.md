# Journey: Administrator Manual Invalidation

This journey describes how an administrator can manually force all users to refresh their local asset cache.

1. **Access Settings**: The administrator navigates to **Website Configuration > Settings**.
2. **Locate Caching Block**: They find the **Caching Service Worker** section ([@ANCHOR: COMM_xpath_rendering_caching_settings]).
3. **Trigger Invalidation**: The administrator clicks the **Invalidate Cache Now** button.
4. **Backend Update**:
   - The system retrieves the current `caching.invalidation_version` ([@ANCHOR: COMM_test_caching_sudo_params]).
   - It increments the version number and saves it back to system parameters.
5. **Reload**: The browser window reloads to confirm the change.
6. **Service Worker Update**:
   - The next time any user's browser requests `/sw.js`, the server injects the new version number into the `CACHE_NAME` ([@ANCHOR: COMM_caching_sw_serve_route]).
   - The browser detects the change in the SW script and installs the new version.
7. **Cache Purge**: During the `activate` phase of the new Service Worker, all old caches that do not match the new name are deleted, ensuring the user gets fresh assets.
