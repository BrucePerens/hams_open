const CACHE_NAME = '__CACHE_NAME__';

// Matches /web/assets/ OR /any_module_name/static/
const CACHE_URL_REGEX = /(\/web\/assets\/|\/[a-zA-Z0-9_-]+\/static\/)/;

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

    if (request.method !== 'GET') return;
    if (url.protocol === 'ws:' || url.protocol === 'wss:') return;
    if (url.pathname.startsWith('/my/') || url.pathname.startsWith('/api/')) return;

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
                        cache.put(request, responseToCache).catch(err => {
                            // Non-fatal, just log and continue
                            console.error(`[Caching SW] Failed to cache ${request.url}:`, err);
                        });
                    }).catch(err => {
                        // Non-fatal, just log and continue
                        console.error(`[Caching SW] Failed to open cache ${CACHE_NAME}:`, err);
                    });
                    return networkResponse;
                });
            })
        );
    }
});
