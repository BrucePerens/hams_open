/** Copyright © HAMS project. AGPL-3.0-or-later. **/
/** @odoo-module **/
import { registry } from "@web/core/registry";
import { TourUtils } from "@zero_sudo/js/tour_utils";

// Closes the gap docs/proposals/SERVICE_WORKER_TESTING.md describes:
// caching_service_worker_check (caching_tour.js) only asserts the SW
// *registers* -- nothing exercises what it actually does once registered
// (intercept fetch, populate Cache Storage, evict stale bundles, enforce
// the storage quota). This tour drives that through the real, registered
// SW in a real browser, the same way every other tour in this codebase
// exercises real Odoo behavior rather than mocking it.
registry.category("web_tour.tours").add("caching_sw_behavior_check", {
    url: "/?debug=1",
    steps: () => [
        {
            content: "Wait for page to load",
            trigger: "body",
        },
        {
            content: "Wait for the Service Worker to be ready and controlling this page",
            trigger: "body",
            run: async function () {
                if (!("serviceWorker" in navigator)) {
                    throw new Error("Service Worker is not supported by this browser environment.");
                }
                await TourUtils.assertSettles(
                    navigator.serviceWorker.ready,
                    5000,
                    "navigator.serviceWorker.ready"
                );
                // A freshly-registered SW may not yet be the *controller* of
                // this page (that only happens after the very first
                // navigation following activation) -- fetches this tour
                // makes are only actually intercepted once it is.
                if (!navigator.serviceWorker.controller) {
                    await new Promise((resolve) => {
                        navigator.serviceWorker.addEventListener("controllerchange", resolve, { once: true });
                    });
                }
                document.body.classList.add("sw-ready-and-controlling");
            },
        },
        {
            content: "Wait for SW controller confirmation",
            trigger: "body.sw-ready-and-controlling",
            run: function () {},
        },
        {
            content: "Test 1: fetching a real cacheable asset actually populates Cache Storage",
            trigger: "body",
            run: async function () {
                // Tests [@ANCHOR: COMM_caching_sw_fetch_interceptor]
                const swSource = await (await fetch("/sw.js")).text();
                const cacheNameMatch = swSource.match(/const CACHE_NAME = '([^']+)';/);
                if (!cacheNameMatch) {
                    throw new Error("Could not extract the real (server-substituted) CACHE_NAME from /sw.js.");
                }
                const cacheName = cacheNameMatch[1];
                window.__swTestCacheName = cacheName;

                // A real static asset this page already loaded, matched
                // against sw.js's own CACHE_URL_REGEX -- discovered from
                // the page rather than hardcoded, so this doesn't rot the
                // moment a bundle hash changes.
                const assetScript = Array.from(document.querySelectorAll("script[src]"))
                    .map((s) => s.getAttribute("src"))
                    .find((src) => src && /^(\/web\/assets\/|\/[a-zA-Z0-9_-]+\/static\/)/.test(src));
                if (!assetScript) {
                    throw new Error("Could not find any script[src] on the page matching sw.js's CACHE_URL_REGEX to test against.");
                }

                await TourUtils.assertSettles(fetch(assetScript), 5000, `fetch(${assetScript})`);

                const cache = await TourUtils.assertSettles(
                    caches.open(cacheName),
                    2000,
                    `caches.open(${cacheName})`
                );
                // The fetch handler caches asynchronously after responding,
                // so give it a short, bounded window to finish rather than
                // racing it -- assertSettles still guarantees this fails
                // fast and specifically if it never does.
                let cached = null;
                const deadline = Date.now() + 3000;
                while (!cached && Date.now() < deadline) {
                    cached = await cache.match(assetScript);
                    if (!cached) {
                        await new Promise((resolve) => setTimeout(resolve, 100));
                    }
                }
                if (!cached) {
                    throw new Error(`Expected ${assetScript} to be present in Cache Storage (${cacheName}) after fetching it through the SW-intercepted path, but it was not.`);
                }
                document.body.classList.add("sw-test-01-passed");
            },
        },
        {
            content: "Confirm test 1 passed",
            trigger: "body.sw-test-01-passed",
            run: function () {},
        },
        {
            content: "Test 2: caching a new bundle hash evicts a stale entry for the same bundle file",
            trigger: "body",
            run: async function () {
                // Tests the "SURGICAL ODOO EVICTION" branch inside
                // [@ANCHOR: COMM_caching_sw_fetch_interceptor]: caching a
                // fresh /web/assets/<hash>/<file> response deletes any
                // other cached entry for the same trailing <file> under a
                // different hash.
                const cacheName = window.__swTestCacheName;
                const cache = await TourUtils.assertSettles(
                    caches.open(cacheName),
                    2000,
                    `caches.open(${cacheName})`
                );

                // A real bundle URL, discovered from the page the same way
                // test 1 discovers its asset -- must be a bundle
                // (/web/assets/...), since only bundles go through the
                // eviction branch (isBundle in sw.js).
                const bundleScript = Array.from(document.querySelectorAll("script[src]"))
                    .map((s) => s.getAttribute("src"))
                    .find((src) => src && /^\/web\/assets\//.test(src));
                if (!bundleScript) {
                    throw new Error("Could not find any /web/assets/ bundle script[src] on the page to test eviction against.");
                }
                const bundleUrl = new URL(bundleScript, document.location.origin);
                const match = bundleUrl.pathname.match(/^\/web\/assets\/[^/]+\/(.+)$/);
                if (!match) {
                    throw new Error(`Bundle script src ${bundleScript} did not match the expected /web/assets/<hash>/<file> shape.`);
                }
                const bundleFile = match[1];

                // A synthetic "stale" entry for the SAME bundle file under a
                // different, fake hash -- seeded directly into Cache
                // Storage (bypassing the SW entirely), simulating a
                // previous deploy's now-superseded bundle that ought to be
                // evicted the next time this exact bundle file is cached
                // under a new hash.
                const staleUrl = `${document.location.origin}/web/assets/__stale_test_hash__/${bundleFile}`;
                await TourUtils.assertSettles(
                    cache.put(
                        staleUrl,
                        new Response("/* stale test bundle */", {
                            status: 200,
                            headers: { "Content-Type": "application/javascript" },
                        })
                    ),
                    2000,
                    `cache.put(${staleUrl})`
                );
                if (!(await cache.match(staleUrl))) {
                    throw new Error("Failed to seed the synthetic stale bundle entry into Cache Storage.");
                }

                // Remove the real bundle's own current entry (if test 1 or
                // normal page load already cached it) so the upcoming
                // fetch is a genuine cache miss and actually exercises the
                // network-fetch + eviction branch, not the cache-hit
                // short-circuit.
                await cache.delete(bundleUrl.pathname);

                await TourUtils.assertSettles(fetch(bundleUrl.pathname), 5000, `fetch(${bundleUrl.pathname})`);

                // Eviction runs asynchronously after the real bundle is
                // cached (same fire-and-forget shape as test 1's own
                // caching), so poll with a bounded deadline instead of
                // racing it.
                let staleGone = false;
                let realCached = null;
                const deadline = Date.now() + 5000;
                while ((!staleGone || !realCached) && Date.now() < deadline) {
                    realCached = await cache.match(bundleUrl.pathname);
                    staleGone = !(await cache.match(staleUrl));
                    if (!staleGone || !realCached) {
                        await new Promise((resolve) => setTimeout(resolve, 100));
                    }
                }
                if (!realCached) {
                    throw new Error(`Expected ${bundleUrl.pathname} to be present in Cache Storage after fetching it through the SW-intercepted path, but it was not.`);
                }
                if (!staleGone) {
                    throw new Error(`Expected the stale same-filename bundle entry (${staleUrl}) to be evicted after caching the new hash, but it is still present.`);
                }
                document.body.classList.add("sw-test-02-passed");
            },
        },
        {
            content: "Confirm test 2 passed",
            trigger: "body.sw-test-02-passed",
            run: function () {},
        },
    ],
});

// A separate tour (not appended to the one above) because it needs its own
// fresh page load with website.caching_safe_quota_mb already set very low
// *before* /sw.js is first fetched (see test_tour.py's
// test_caching_sw_lru_eviction_tour) -- MAX_STORAGE_BYTES is baked into
// the served /sw.js at fetch time, not re-read live, so it has to be small
// from this tour's very first navigation. Running it against the normal
// default quota (like the tour above) would mean seeding enough real bytes
// to exceed tens of megabytes just to test eviction, which is both slow
// and needlessly fragile.
registry.category("web_tour.tours").add("caching_sw_lru_eviction_check", {
    url: "/?debug=1",
    steps: () => [
        {
            content: "Wait for page to load",
            trigger: "body",
        },
        {
            content: "Wait for the Service Worker to be ready and controlling this page",
            trigger: "body",
            run: async function () {
                if (!("serviceWorker" in navigator)) {
                    throw new Error("Service Worker is not supported by this browser environment.");
                }
                await TourUtils.assertSettles(
                    navigator.serviceWorker.ready,
                    5000,
                    "navigator.serviceWorker.ready"
                );
                if (!navigator.serviceWorker.controller) {
                    await new Promise((resolve) => {
                        navigator.serviceWorker.addEventListener("controllerchange", resolve, { once: true });
                    });
                }
                document.body.classList.add("sw-ready-and-controlling");
            },
        },
        {
            content: "Wait for SW controller confirmation",
            trigger: "body.sw-ready-and-controlling",
            run: function () {},
        },
        {
            content: "Test 3: exceeding the storage quota evicts the oldest LRU entries",
            trigger: "body",
            run: async function () {
                // Tests enforceLRUQuota() in sw.js -- the exact code path
                // the missing-onerror-handler hang bug (see
                // docs/proposals/SERVICE_WORKER_TESTING.md's "Done"
                // section) lived in, still with zero behavioral coverage
                // before this test.
                const swSource = await (await fetch("/sw.js")).text();
                const cacheNameMatch = swSource.match(/const CACHE_NAME = '([^']+)';/);
                if (!cacheNameMatch) {
                    throw new Error("Could not extract the real (server-substituted) CACHE_NAME from /sw.js.");
                }
                const cacheName = cacheNameMatch[1];
                const maxStorageMatch = swSource.match(/const MAX_STORAGE_BYTES = (\d+);/);
                if (!maxStorageMatch) {
                    throw new Error("Could not extract MAX_STORAGE_BYTES from /sw.js.");
                }
                const maxStorageBytes = parseInt(maxStorageMatch[1], 10);

                const cache = await TourUtils.assertSettles(
                    caches.open(cacheName),
                    2000,
                    `caches.open(${cacheName})`
                );

                // Mirrors sw.js's own openDB() exactly, including the
                // onupgradeneeded store creation -- this tour's first
                // fetch-triggering step is Test 3 itself, so on a fresh
                // browser profile the SW hasn't opened (and therefore
                // hasn't created) LRUCacheDB yet by the time this runs;
                // without this, "indexedDB.open(LRUCacheDB, 1)" here would
                // win the race and create an empty DB with no
                // "LRUMetadata" object store, and every transaction below
                // would fail with NotFoundError.
                const openLRUDb = () =>
                    new Promise((resolve, reject) => {
                        const req = indexedDB.open("LRUCacheDB", 1);
                        req.onupgradeneeded = (event) => {
                            const idb = event.target.result;
                            if (!idb.objectStoreNames.contains("LRUMetadata")) {
                                const store = idb.createObjectStore("LRUMetadata", { keyPath: "url" });
                                store.createIndex("timestamp", "timestamp", { unique: false });
                            }
                        };
                        req.onsuccess = () => resolve(req.result);
                        req.onerror = () => reject(req.error);
                    });
                const db = await TourUtils.assertSettles(openLRUDb(), 2000, "indexedDB.open(LRUCacheDB)");

                // Directly seed several oversized synthetic cache entries
                // (well past maxStorageBytes combined) with ascending
                // timestamps in LRUMetadata -- simulating a cache that's
                // already over quota before this test's own triggering
                // fetch, exactly the state enforceLRUQuota() exists to
                // clean up. Seeded directly into Cache Storage/IndexedDB
                // (bypassing the SW's fetch handler), since the point is
                // to control the *before* state precisely, not to test how
                // entries got cached in the first place (tests 1/2 in the
                // sibling tour already cover that).
                const bigBody = "x".repeat(Math.max(1024, Math.ceil(maxStorageBytes / 2)));
                const entryUrls = [];
                for (let i = 0; i < 4; i++) {
                    const url = `${document.location.origin}/caching/static/lru_eviction_test_${i}.js`;
                    entryUrls.push(url);
                    await cache.put(
                        url,
                        new Response(bigBody, { status: 200, headers: { "Content-Type": "application/javascript" } })
                    );
                    await new Promise((resolve, reject) => {
                        const tx = db.transaction("LRUMetadata", "readwrite");
                        tx.objectStore("LRUMetadata").put({ url, timestamp: 1000 + i });
                        tx.oncomplete = resolve;
                        tx.onerror = () => reject(tx.error);
                    });
                }

                // Confirm we're actually over quota now, or the rest of
                // this test wouldn't mean anything -- checked the same way
                // enforceLRUQuota() itself checks.
                const estimate = await navigator.storage.estimate();
                if (estimate.usage <= maxStorageBytes) {
                    throw new Error(
                        `Test setup did not actually exceed MAX_STORAGE_BYTES (usage=${estimate.usage}, ` +
                            `max=${maxStorageBytes}) -- cannot exercise enforceLRUQuota().`
                    );
                }

                // Fetch one real, non-bundle static asset through the
                // SW-intercepted path -- enforceLRUQuota() only runs after
                // a successful non-bundle cache.put() (see sw.js's fetch
                // handler: "if (!isBundle) { updateLRUMetadata(...).then(()
                // => enforceLRUQuota(cache)) }"). sw.js's own
                // CACHE_URL_REGEX matches any /<module>/static/... path
                // regardless of whether the current page happens to link
                // it via a <script src> tag -- this page (the front-end
                // "/") doesn't render any bare (non-bundled) module static
                // script tags at all, so searching the DOM for one (as
                // tests 1/2 do for their own, different assets) finds
                // nothing here. sw.js itself is a real, always-present
                // asset under /caching/static/ that isn't under
                // /web/assets/, so fetch it directly instead.
                const assetScript = "/caching/static/src/sw/sw.js";
                // Force a cache miss so the fetch actually goes to network
                // and re-caches (a cache hit wouldn't retrigger eviction).
                await cache.delete(assetScript);
                await TourUtils.assertSettles(fetch(assetScript), 5000, `fetch(${assetScript})`);

                // enforceLRUQuota() deletes the oldest 10 IndexedDB-tracked
                // entries by timestamp in one batch; only 4 were seeded, so
                // all 4 (the oldest entries in the whole store) should be
                // gone. Runs asynchronously after the triggering fetch is
                // cached, so poll with a bounded deadline instead of
                // racing it.
                let allGone = false;
                const deadline = Date.now() + 5000;
                while (!allGone && Date.now() < deadline) {
                    const remaining = await Promise.all(entryUrls.map((u) => cache.match(u)));
                    allGone = remaining.every((r) => !r);
                    if (!allGone) {
                        await new Promise((resolve) => setTimeout(resolve, 150));
                    }
                }
                if (!allGone) {
                    throw new Error(
                        "Expected all 4 seeded over-quota LRU entries to be evicted by enforceLRUQuota(), " +
                            "but at least one is still present."
                    );
                }
                document.body.classList.add("sw-test-03-passed");
            },
        },
        {
            content: "Confirm test 3 passed",
            trigger: "body.sw-test-03-passed",
            run: function () {},
        },
    ],
});

// docs/proposals/SERVICE_WORKER_TESTING.md's "Still open" item: forcing a
// *synthetic* IndexedDB failure needs more than page-context
// monkeypatching, since the page and the Service Worker run in separate
// global scopes with their own `indexedDB` -- patching
// `window.indexedDB.open` from a tour step has no effect on the SW's own
// calls. sw.js's own 'message' listener (TEST_FORCE_IDB_ERROR/
// TEST_CALL_OPEN_DB) is the real fix; this drives it through a real
// registered SW to prove openDB() -- the exact function whose missing
// onerror handler used to hang enforceLRUQuota()/updateLRUMetadata()
// forever with no diagnostic -- now actually settles (rejects) instead.
async function sendTestMessageToSW(registration, message) {
    return new Promise((resolve, reject) => {
        const channel = new MessageChannel();
        channel.port1.onmessage = (event) => resolve(event.data);
        channel.port1.onmessageerror = (event) => reject(new Error(`postMessage channel error: ${event}`));
        registration.active.postMessage(message, [channel.port2]);
    });
}

registry.category("web_tour.tours").add("caching_sw_idb_error_check", {
    url: "/?debug=1",
    steps: () => [
        {
            content: "Wait for page to load",
            trigger: "body",
        },
        {
            content: "Wait for the Service Worker to be ready",
            trigger: "body",
            run: async function () {
                if (!("serviceWorker" in navigator)) {
                    throw new Error("Service Worker is not supported by this browser environment.");
                }
                const registration = await TourUtils.assertSettles(
                    navigator.serviceWorker.ready,
                    5000,
                    "navigator.serviceWorker.ready"
                );
                window.__testSwRegistration = registration;
                document.body.classList.add("sw-idb-test-ready");
            },
        },
        {
            content: "Confirm SW ready",
            trigger: "body.sw-idb-test-ready",
            run: function () {},
        },
        {
            content: "Test: openDB() settles normally (resolves) before forcing an error",
            trigger: "body",
            run: async function () {
                // Tests [@ANCHOR: sw_idb_open_db_normal]
                const result = await TourUtils.assertSettles(
                    sendTestMessageToSW(window.__testSwRegistration, { type: "TEST_CALL_OPEN_DB" }),
                    5000,
                    "openDB() (normal path)"
                );
                if (result.rejected) {
                    throw new Error(`Expected openDB() to resolve normally before forcing an error, but it rejected: ${result.message}`);
                }
                document.body.classList.add("sw-idb-test-normal-passed");
            },
        },
        {
            content: "Confirm normal openDB() test passed",
            trigger: "body.sw-idb-test-normal-passed",
            run: function () {},
        },
        {
            content: "Test: a forced IndexedDB failure makes openDB() reject, not hang",
            trigger: "body",
            run: async function () {
                // Tests [@ANCHOR: sw_idb_open_db_forced_error]
                await TourUtils.assertSettles(
                    sendTestMessageToSW(window.__testSwRegistration, { type: "TEST_FORCE_IDB_ERROR" }),
                    5000,
                    "TEST_FORCE_IDB_ERROR"
                );
                // The real assertion: before the onerror-handler fix, this
                // exact call pattern (an IndexedDB request failing) left
                // its Promise permanently unsettled -- assertSettles'
                // bounded timeout is what turns a regression back to that
                // bug into a specific test failure instead of a silent
                // hang.
                const result = await TourUtils.assertSettles(
                    sendTestMessageToSW(window.__testSwRegistration, { type: "TEST_CALL_OPEN_DB" }),
                    5000,
                    "openDB() (forced-error path)"
                );
                if (!result.rejected) {
                    throw new Error("Expected openDB() to reject once TEST_FORCE_IDB_ERROR was set, but it resolved.");
                }
                if (!result.message || !result.message.includes("TEST_FORCED_IDB_ERROR")) {
                    throw new Error(`Expected the TEST_FORCED_IDB_ERROR message, got: ${result.message}`);
                }
                document.body.classList.add("sw-idb-test-forced-error-passed");
            },
        },
        {
            content: "Confirm forced-error openDB() test passed",
            trigger: "body.sw-idb-test-forced-error-passed",
            run: function () {},
        },
    ],
});
