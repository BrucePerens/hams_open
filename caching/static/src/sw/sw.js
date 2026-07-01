/** @odoo-module **/

const CACHE_NAME = '__CACHE_NAME__';

// Matches /web/assets/ OR /any_module_name/static/
// Anchored to the start of the path for precision.
const CACHE_URL_REGEX = /^(\/web\/assets\/|\/[a-zA-Z0-9_-]+\/static\/)/;

// Dynamically calculated by the Python backend to prevent quota exhaustion
const MAX_FILE_SIZE_BYTES = __MAX_FILE_SIZE_BYTES__;
const MAX_STORAGE_BYTES = __MAX_STORAGE_BYTES__;

const DB_NAME = 'LRUCacheDB';
const STORE_NAME = 'LRUMetadata';

function openDB() {
    return new Promise((resolve, reject) => {
        const request = indexedDB.open(DB_NAME, 1);
        request.onupgradeneeded = (event) => {
            const db = event.target.result;
            if (!db.objectStoreNames.contains(STORE_NAME)) {
                const store = db.createObjectStore(STORE_NAME, { keyPath: 'url' });
                store.createIndex('timestamp', 'timestamp', { unique: false });
            }
        };
        request.onsuccess = () => resolve(request.result);
        request.onerror = () => reject(request.error);
    });
}

async function updateLRUMetadata(url) {
    try {
        const db = await openDB();
        return new Promise((resolve, reject) => {
            const tx = db.transaction(STORE_NAME, 'readwrite');
            const store = tx.objectStore(STORE_NAME);
            store.put({ url: url, timestamp: Date.now() });
            tx.oncomplete = () => resolve();
            tx.onerror = () => reject(tx.error);
        });
    } catch (e) {
        console.error('[Caching SW] IDB update error:', e);
    }
}

async function enforceLRUQuota(cache) {
    try {
        if (!navigator.storage || !navigator.storage.estimate) return;
        
        const estimate = await navigator.storage.estimate();
        
        if (estimate.usage <= MAX_STORAGE_BYTES) return;

        const db = await openDB();
        return new Promise((resolve, reject) => {
            const tx = db.transaction(STORE_NAME, 'readwrite');
            const store = tx.objectStore(STORE_NAME);
            const index = store.index('timestamp');
            
            // Delete oldest 10 items as a batch to quickly free up space
            let toDelete = 10; 
            const request = index.openCursor();
            request.onsuccess = (event) => {
                const cursor = event.target.result;
                if (cursor && toDelete > 0) {
                    cache.delete(cursor.value.url).catch(console.error);
                    cursor.delete();
                    toDelete--;
                    cursor.continue();
                } else {
                    resolve();
                }
            };
        });
    } catch (e) {
        console.error('[Caching SW] IDB quota enforcement error:', e);
    }
}

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

    // Explicitly bypass documentation images
    if (url.pathname.includes('/static/description/images/')) return;

    // We only intercept requests that match our static asset patterns.
    if (CACHE_URL_REGEX.test(url.pathname)) {
        event.respondWith(
            caches.match(request).then((cachedResponse) => {
                if (cachedResponse) {
                    const isBundle = url.pathname.startsWith('/web/assets/');
                    if (!isBundle) updateLRUMetadata(request.url);
                    return cachedResponse;
                }
                return fetch(request).then((networkResponse) => {
                    if (!networkResponse || networkResponse.status !== 200 || networkResponse.type !== 'basic') {
                        return networkResponse;
                    }

                    // SIZE LIMIT SAFETY VALVE
                    // Odoo bundles are exempt from the dynamic module quota,
                    // as they fit within the 10MB system reservation.
                    const isBundle = url.pathname.startsWith('/web/assets/');
                    const contentLength = networkResponse.headers.get('Content-Length');
                    const parsedLength = contentLength ? parseInt(contentLength, 10) : NaN;
                    
                    if (!isBundle && (!isNaN(parsedLength) && parsedLength > MAX_FILE_SIZE_BYTES)) {
                        console.warn(`[Caching SW] Skipping cache for large file: ${request.url}`);
                        return networkResponse;
                    }

                    const responseToCache = networkResponse.clone();
                    caches.open(CACHE_NAME).then((cache) => {
                        cache.put(request, responseToCache).then(() => {
                            if (!isBundle) {
                                updateLRUMetadata(request.url).then(() => enforceLRUQuota(cache));
                            }
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
