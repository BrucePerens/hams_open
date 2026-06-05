const CACHE_NAME = '__CACHE_NAME__';

// Matches /web/assets/ OR /any_module_name/static/
// Anchored to the start of the path for precision.
const CACHE_URL_REGEX = /^(\/web\/assets\/|\/[a-zA-Z0-9_-]+\/static\/)/;

// Dynamically calculated by the Python backend to prevent quota exhaustion
const MAX_FILE_SIZE_BYTES = __MAX_FILE_SIZE_BYTES__;

self.addEventListener('install', (event) => {
    self.skipWaiting();
});

self.addEventListener('activate', (event) => {
    event.waitUntil(
        caches.keys().then((cacheNames) => {
            return Promise.all(
                cacheNames.map((cacheName) => {
                    if (cacheName !== CACHE_NAME) {
                        return caches.delete(cacheName);
                    }
                })
            );
        }).then(() => self.clients.claim())
    );
});

self.addEventListener('fetch', (event) => {
    // [@ANCHOR: caching_sw_fetch_interceptor]
    // Verified by [@ANCHOR: test_sw_fetch_01]
    const request = event.request;
    const url = new URL(request.url);

    // Only cache GET requests.
    if (request.method !== 'GET') return;

    // BYPASS: Chrome only-if-cached bug which throws TypeErrors and forces Odoo retry loops
    if (request.cache === 'only-if-cached' && request.mode !== 'same-origin') return;

    // Explicitly bypass WebSockets, secure APIs, and dynamic routes.
    if (url.protocol === 'ws:' || url.protocol === 'wss:') return;
    if (url.pathname.startsWith('/my/') || url.pathname.startsWith('/api/') || url.pathname.startsWith('/web/image/') || url.pathname.startsWith('/web/content/')) return; // burn-ignore-route

    // We only intercept requests that match our static asset patterns.
    if (CACHE_URL_REGEX.test(url.pathname)) {
        event.respondWith(
            caches.match(request).then((cachedResponse) => {
                if (cachedResponse) {
                    return cachedResponse;
                }
                return fetch(request).then((networkResponse) => {
                    if (!networkResponse || networkResponse.status !== 200 || networkResponse.type !== 'basic') {
                        return networkResponse;
                    }

                    // SIZE LIMIT SAFETY VALVE
                    const contentLength = networkResponse.headers.get('Content-Length');
                    if (contentLength && parseInt(contentLength, 10) > MAX_FILE_SIZE_BYTES) {
                        console.warn(`[Caching SW] Skipping cache for large file: ${request.url} (${contentLength} bytes)`);
                        return networkResponse;
                    }

                    const responseToCache = networkResponse.clone();
                    caches.open(CACHE_NAME).then((cache) => {
                        cache.put(request, responseToCache).then(() => {
                            // SURGICAL ODOO EVICTION:
                            // Odoo bundles follow /web/assets/<hash>/<bundle_name>.js
                            // If we cache a new hash, instantly delete the old hash for the same bundle to prevent gigabytes of cache bloat.
                            const match = url.pathname.match(/\/web\/assets\/[^\/]+\/(.+)/);
                            if (match) {
                                const bundleFile = match[1];
                                cache.keys().then(keys => {
                                    keys.forEach(key => {
                                        const keyUrl = new URL(key.url);
                                        const keyMatch = keyUrl.pathname.match(/\/web\/assets\/[^\/]+\/(.+)/);
                                        if (keyMatch && keyMatch[1] === bundleFile && keyUrl.pathname !== url.pathname) {
                                            cache.delete(key);
                                            console.log(`[Caching SW] Evicted stale Odoo bundle: ${keyUrl.pathname}`);
                                        }
                                    });
                                });
                            }
                        }).catch(err => {
                            console.error(`[Caching SW] Failed to cache ${request.url}:`, err);
                        });
                    }).catch(err => {
                        console.error(`[Caching SW] Failed to open cache ${CACHE_NAME}:`, err);
                    });

                    return networkResponse;
                });
            })
        );
    }
});
