/** Copyright © HAMS project. AGPL-3.0. **/
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
                        await new Promise((r) => setTimeout(r, 100));
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
    ],
});
