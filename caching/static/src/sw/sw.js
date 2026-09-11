/** Copyright © HAMS project. AGPL-3.0-or-later. **/
/** @odoo-module **/

const CACHE_NAME = '__CACHE_NAME__';

// Every cache name this SW ever creates starts with this prefix (see
// caching/controllers/main.py's own `cache_name = f"odoo-assets-cache-..."`).
// activate() below must only ever delete OUR OWN stale caches, never any
// other Cache Storage entry on this origin -- shack_sw.js (a different
// Service Worker, but the SAME origin's shared CacheStorage) keeps its own
// "ham-shack-offline-*" caches there, and an unscoped `cacheName !==
// CACHE_NAME` delete would wipe them out on every single caching-module
// version bump.
const CACHE_NAME_PREFIX = 'odoo-assets-cache-';

// Matches /web/assets/ OR /any_module_name/static/
// Anchored to the start of the path for precision.
const CACHE_URL_REGEX = /^(\/web\/assets\/|\/[a-zA-Z0-9_-]+\/static\/)/;

// Dynamically calculated by the Python backend to prevent quota exhaustion
const MAX_FILE_SIZE_BYTES = __MAX_FILE_SIZE_BYTES__;
const MAX_STORAGE_BYTES = __MAX_STORAGE_BYTES__;

const DB_NAME = 'LRUCacheDB';
const STORE_NAME = 'LRUMetadata';

// A real per-deployment operational switch, substituted server-side by
// caching/controllers/main.py's own /sw.js route from the
// caching.enable_sw_test_hooks config parameter -- see that route's own
// comment and security_utils.py's whitelist entry for why this is
// deliberately NOT tied to Odoo's test_enable flag. False (hooks compiled
// out of what's served) on every deployment unless a human has explicitly
// turned it on for that specific instance; the tour tests below only ever
// run against an instance that has.
const TEST_HOOKS_ENABLED = __TEST_HOOKS_ENABLED__;

// Test-only: forces the next openDB() call to reject instead of opening a
// real database, for tour-driven coverage of the error paths that a real
// IndexedDB failure exercises. Never set true outside a test -- there is
// no production code path that sets this. The page and this Service
// Worker run in separate global scopes with their own `indexedDB`, so
// this can't be forced by monkeypatching `window.indexedDB.open` from a
// tour step; it has to be a real flag inside the SW itself, toggled via
// postMessage (see the 'message' listener below).
// docs/proposals/SERVICE_WORKER_TESTING.md
let __testForceIdbError = false;

function openDB() {
    return new Promise((resolve, reject) => {
        if (__testForceIdbError) {
            reject(new Error('TEST_FORCED_IDB_ERROR'));
            return;
        }
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
            tx.onabort = () => reject(tx.error || new Error('Transaction aborted'));
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
            request.onerror = () => reject(request.error);
            tx.onerror = () => reject(tx.error);
            tx.onabort = () => reject(tx.error || new Error('Transaction aborted'));
        });
    } catch (e) {
        console.error('[Caching SW] IDB quota enforcement error:', e);
    }
}

// Last-resort fallback when even the precached '/offline' entry is
// missing (evicted, or install-time precache itself failed) -- without
// this, caches.match() resolving to undefined makes respondWith(undefined)
// a hard network error instead of a usable offline page.
// [@ANCHOR: caching_sw_offline_fallback_response]
function offlineFallbackResponse() {
    return new Response(
        '<!doctype html><html><head><meta charset="utf-8"><title>Offline</title></head>' +
        '<body><h1>You are offline</h1><p>This page is not available right now. ' +
        'Reconnect to the network and reload.</p></body></html>',
        { status: 503, headers: { 'Content-Type': 'text/html' } }
    );
}

self.addEventListener('install', (event) => {
    self.skipWaiting();
    event.waitUntil(
        caches.open(CACHE_NAME).then((cache) => {
            return cache.addAll([
                '/offline'
            ]);
        })
    );
});

// Deletes only OUR OWN stale caches (see CACHE_NAME_PREFIX's own comment).
// Factored out of the 'activate' listener so a test can invoke it directly
// via the TEST_RUN_CACHE_CLEANUP message hook below, without needing to
// force a real install/activate cycle (a new SW version replacing this
// one) just to exercise this logic.
// [@ANCHOR: caching_sw_cache_cleanup_prefix_scoped]
function cleanupStaleCaches() {
    return caches.keys().then((cacheNames) => {
        return Promise.all(
            cacheNames.map((cacheName) => {
                if (cacheName.startsWith(CACHE_NAME_PREFIX) && cacheName !== CACHE_NAME) {
                    return caches.delete(cacheName);
                }
            })
        );
    });
}

self.addEventListener('activate', (event) => {
    event.waitUntil(
        cleanupStaleCaches().then(() => self.clients.claim()).then(() => self.clients.matchAll()).then((clients) => {
            clients.forEach(client => client.postMessage({ type: 'NEW_VERSION_INSTALLED' }));
            return;
        })
    );
});

self.addEventListener('fetch', (event) => {
    // [@ANCHOR: COMM_caching_sw_fetch_interceptor]

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

    // Network-first for navigations with offline fallback
    if (request.mode === 'navigate') {
        event.respondWith(
            fetch(request).catch(async () => (await caches.match('/offline')) || offlineFallbackResponse())
        );
        return;
    }

    // We only intercept requests that match our static asset patterns.
    if (CACHE_URL_REGEX.test(url.pathname)) {
        event.respondWith((async () => {
            const cachedResponse = await caches.match(request);
            if (cachedResponse) {
                const isBundle = url.pathname.startsWith('/web/assets/');
                if (!isBundle) void updateLRUMetadata(request.url);
                return cachedResponse;
            }

            const networkResponse = await fetch(request);
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
            // Deliberately not awaited: caching the response must not delay
            // returning networkResponse to the page. Errors are logged, not
            // propagated -- a cache-write failure shouldn't fail the fetch.
            void (async () => {
                let cache;
                try {
                    cache = await caches.open(CACHE_NAME);
                } catch (err) {
                    console.error(`[Caching SW] Failed to open cache ${CACHE_NAME}:`, err);
                    return;
                }
                try {
                    if (!isBundle && isNaN(parsedLength)) {
                        // Content-Length was absent above (a chunked or
                        // compressed response -- most JS/CSS in practice),
                        // which used to skip the size check entirely and
                        // let an arbitrarily large asset bypass
                        // MAX_FILE_SIZE_BYTES. Measure the real decoded
                        // body size here instead, off the response path
                        // (this whole block is unawaited already).
                        let sizeBytes;
                        try {
                            sizeBytes = (await responseToCache.clone().arrayBuffer()).byteLength;
                        } catch (err) {
                            console.warn(`[Caching SW] Could not measure size for ${request.url}, skipping cache:`, err);
                            return;
                        }
                        if (sizeBytes > MAX_FILE_SIZE_BYTES) {
                            console.warn(`[Caching SW] Skipping cache for large file (measured): ${request.url}`);
                            return;
                        }
                    }
                    try {
                        // Keep responseToCache itself unconsumed (clone for
                        // the actual put) so a QuotaExceededError below can
                        // retry from a fresh, unread body instead of trying
                        // to re-read an already-consumed stream.
                        await cache.put(request, responseToCache.clone());
                    } catch (putErr) {
                        // A full origin quota makes cache.put() itself throw
                        // QuotaExceededError. enforceLRUQuota() previously
                        // only ever ran AFTER a successful put, so once the
                        // cache was actually full, nothing ever evicted
                        // anything again -- permanently stuck full. Evict
                        // once and retry this exact put a single time.
                        if (putErr && putErr.name === 'QuotaExceededError' && !isBundle) {
                            await enforceLRUQuota(cache);
                            await cache.put(request, responseToCache.clone());
                        } else {
                            throw putErr;
                        }
                    }
                    if (!isBundle) {
                        updateLRUMetadata(request.url).then(() => enforceLRUQuota(cache)).catch(console.error);
                    }
                    // SURGICAL ODOO EVICTION:
                    // Odoo bundles follow /web/assets/<hash>/<bundle_name>.js
                    // If we cache a new hash, instantly delete the old hash for the same bundle to prevent gigabytes of cache bloat.
                    const match = url.pathname.match(/\/web\/assets\/[^/]+\/(.+)/);
                    if (match) {
                        const bundleFile = match[1];
                        const keys = await cache.keys();
                        keys.forEach(key => {
                            const keyUrl = new URL(key.url);
                            const keyMatch = keyUrl.pathname.match(/\/web\/assets\/[^/]+\/(.+)/);
                            if (keyMatch && keyMatch[1] === bundleFile && keyUrl.pathname !== url.pathname) {
                                void cache.delete(key);
                                console.log(`[Caching SW] Evicted stale Odoo bundle: ${keyUrl.pathname}`);
                            }
                        });
                    }
                } catch (err) {
                    console.error(`[Caching SW] Failed to cache ${request.url}:`, err);
                }
            })();

            return networkResponse;
        })());
    }
});

// Test-only postMessage hooks -- see openDB()'s own comment on
// __testForceIdbError above. `event.ports[0]` is the MessageChannel port
// a tour step provides so it can await confirmation the flag actually
// took effect (or, for TEST_CALL_OPEN_DB, the actual settlement) inside
// this SW's own scope, rather than assuming the postMessage was
// delivered and processed before the next test step runs.
self.addEventListener('message', (event) => {
    if (!event.data || !event.data.type) return;
    // See TEST_HOOKS_ENABLED's own comment above -- these are all
    // TEST_-prefixed by convention, and none of them is meaningful
    // production behavior, so gating the whole family at once here is
    // safe and doesn't touch anything a real client would ever send.
    if (!TEST_HOOKS_ENABLED) return;

    if (event.data.type === 'TEST_FORCE_IDB_ERROR') {
        __testForceIdbError = true;
        if (event.ports && event.ports[0]) {
            event.ports[0].postMessage({ ok: true });
        }
        return;
    }

    if (event.data.type === 'TEST_RUN_CACHE_CLEANUP') {
        // Re-runs activate()'s own cache-cleanup logic on demand -- proves
        // it only ever deletes caches under CACHE_NAME_PREFIX (this SW's
        // own family) and never a differently-named cache belonging to a
        // different Service Worker sharing this origin's CacheStorage
        // (shack_sw.js's 'ham-shack-offline-*' caches, in particular).
        const port = event.ports && event.ports[0];
        cleanupStaleCaches().then(() => {
            if (port) port.postMessage({ ok: true });
        }).catch((err) => {
            if (port) port.postMessage({ ok: false, message: String(err && err.message) });
        });
        return;
    }

    if (event.data.type === 'TEST_CALL_OPEN_DB') {
        // Calls openDB() directly and reports back whether it settled
        // (resolved or rejected) and, if it rejected, the error message --
        // this is the exact function that had the original hang bug
        // (a cursor request with no onerror handler left its Promise
        // permanently unsettled), so this is what proves the fix holds
        // rather than just reading the code and trusting it. A tour step
        // wraps this call in its own bounded timeout (TourUtils.assertSettles)
        // so a regression back to "never settles" fails the test instead
        // of hanging it.
        const port = event.ports && event.ports[0];
        openDB()
            .then(() => {
                if (port) port.postMessage({ settled: true, rejected: false });
                return;
            })
            .catch((err) => {
                if (port) port.postMessage({ settled: true, rejected: true, message: String(err && err.message) });
            });
        return;
    }

    if (event.data.type === 'TEST_CHECK_OFFLINE_FALLBACK') {
        // Tests [@ANCHOR: caching_sw_offline_fallback_response]
        const port = event.ports && event.ports[0];
        if (port) port.postMessage({ status: offlineFallbackResponse().status });
    }
});
