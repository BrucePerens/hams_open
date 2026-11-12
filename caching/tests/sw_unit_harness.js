// Copyright © HAMS project. AGPL-3.0-or-later.
// Runs one named test of the service worker's own functions: node sw_unit_harness.js <path to sw.js> <test name>.
// The worker source is loaded into a vm context with its server-substituted placeholders filled in and stand-ins for the
// worker globals (self, caches, indexedDB, navigator); exit status 0 means the test passed.
"use strict";
const assert = require("assert");
const fs = require("fs");
const vm = require("vm");

const SOURCE = fs
    .readFileSync(process.argv[2], "utf8")
    .replace("__CACHE_NAME__", "odoo-assets-cache-new")
    .replace("__MAX_FILE_SIZE_BYTES__", "1000")
    .replace("__MAX_STORAGE_BYTES__", "5000")
    .replace("__TEST_HOOKS_ENABLED__", "false");

/** Loads the worker into a fresh context; returns its context (functions are properties of it) and the listeners it registered. */
// [@ANCHOR: caching:js_sw_harness_load]
function load(globals = {}) {
    const listeners = {};
    const logged = [];
    const self = { addEventListener: (name, handler) => { listeners[name] = handler; }, clients: {} };
    const context = vm.createContext({
        self,
        console: { log() {}, warn() {}, error: (...args) => logged.push(args) },
        Date, Promise, Response, URL, setTimeout, queueMicrotask,
        ...globals,
    });
    vm.runInContext(SOURCE, context);
    return { context, listeners, self, logged };
}

/** An indexedDB whose open() answers asynchronously; `stores` are the object stores the database already has. */
// [@ANCHOR: caching:js_sw_harness_fakeIndexedDB]
function fakeIndexedDB({ stores = [], fail = false, transaction } = {}) {
    const seen = { opens: [], created: [] };
    const db = {
        objectStoreNames: { contains: (name) => stores.includes(name) },
        createObjectStore: (name, options) => {
            seen.created.push(["store", name, options]);
            return { createIndex: (...args) => seen.created.push(["index", ...args]) };
        },
        transaction,
    };
    const indexedDB = {
        open: (name, version) => {
            seen.opens.push([name, version]);
            const request = {};
            setTimeout(() => {
                if (fail) {
                    request.error = new Error("open failed");
                    request.onerror();
                    return;
                }
                request.result = db;
                if (!stores.length) {
                    request.onupgradeneeded({ target: { result: db } });
                }
                request.onsuccess();
            }, 0);
            return request;
        },
    };
    return { indexedDB, seen, db };
}

/** Objects made inside the vm have another realm's prototypes; this copies one into this realm for deepStrictEqual. */
// [@ANCHOR: caching:js_sw_harness_plain]
const plain = (value) => JSON.parse(JSON.stringify(value));

/** The error a promise rejects with, or null when it resolves. */
// [@ANCHOR: caching:js_sw_harness_rejection]
async function rejection(promise) {
    try {
        await promise;
        return null;
    } catch (error) {
        return error;
    }
}

const tests = {
    // [@ANCHOR: caching:js_sw_harness_open_db]
    async open_db() {
        const fresh = fakeIndexedDB();
        const { context } = load({ indexedDB: fresh.indexedDB });
        assert.strictEqual(await context.openDB(), fresh.db);
        assert.deepStrictEqual(fresh.seen.opens, [["LRUCacheDB", 1]]);
        assert.deepStrictEqual(plain(fresh.seen.created), [
            ["store", "LRUMetadata", { keyPath: "url" }],
            ["index", "timestamp", "timestamp", { unique: false }],
        ]);
        const existing = fakeIndexedDB({ stores: ["LRUMetadata"] });
        await load({ indexedDB: existing.indexedDB }).context.openDB();
        assert.deepStrictEqual(existing.seen.created, []);
        const broken = fakeIndexedDB({ fail: true });
        const error = await rejection(load({ indexedDB: broken.indexedDB }).context.openDB());
        assert.strictEqual(error.message, "open failed");
    },

    // [@ANCHOR: caching:js_sw_harness_update_lru_metadata]
    async update_lru_metadata() {
        const puts = [];
        const transaction = () => {
            const tx = { objectStore: () => ({ put: (row) => { puts.push(row); setTimeout(() => tx.oncomplete(), 0); } }) };
            return tx;
        };
        const { indexedDB } = fakeIndexedDB({ stores: ["LRUMetadata"], transaction });
        const { context } = load({ indexedDB });
        const before = Date.now();
        await context.updateLRUMetadata("/web/assets/a.js");
        assert.strictEqual(puts.length, 1);
        assert.strictEqual(puts[0].url, "/web/assets/a.js");
        assert.ok(puts[0].timestamp >= before && puts[0].timestamp <= Date.now());
        const broken = fakeIndexedDB({ fail: true });
        const failing = load({ indexedDB: broken.indexedDB });
        assert.strictEqual(await failing.context.updateLRUMetadata("/x"), undefined);
        assert.strictEqual(failing.logged.length, 1);
    },

    // [@ANCHOR: caching:js_sw_harness_enforce_lru_quota]
    async enforce_lru_quota() {
        const cacheDeleted = [];
        const cache = { delete: async (url) => { cacheDeleted.push(url); return true; } };
        const rows = Array.from({ length: 12 }, (_, i) => ({ url: `/web/assets/${i}.js` }));
        const cursorDeleted = [];
        const transaction = (rowsToWalk) => () => ({
            objectStore: () => ({
                index: () => ({
                    openCursor: () => {
                        const request = {};
                        let at = 0;
                        const step = () => {
                            const row = rowsToWalk[at];
                            request.onsuccess({
                                target: { result: row ? { value: row, delete: () => cursorDeleted.push(row.url), continue: () => { at += 1; setTimeout(step, 0); } } : null },
                            });
                        };
                        setTimeout(step, 0);
                        return request;
                    },
                }),
            }),
        });
        // No storage API, or usage within the quota: the database is never opened.
        const untouched = fakeIndexedDB({ stores: ["LRUMetadata"], transaction: transaction(rows) });
        await load({ indexedDB: untouched.indexedDB, navigator: {} }).context.enforceLRUQuota(cache);
        await load({ indexedDB: untouched.indexedDB, navigator: { storage: { estimate: async () => ({ usage: 5000 }) } } }).context.enforceLRUQuota(cache);
        assert.deepStrictEqual(untouched.seen.opens, []);
        // Over the quota: the ten least recently used entries go, from the cache and from the bookkeeping.
        const over = fakeIndexedDB({ stores: ["LRUMetadata"], transaction: transaction(rows) });
        await load({ indexedDB: over.indexedDB, navigator: { storage: { estimate: async () => ({ usage: 5001 }) } } }).context.enforceLRUQuota(cache);
        const oldest = rows.slice(0, 10).map((row) => row.url);
        assert.deepStrictEqual(cacheDeleted, oldest);
        assert.deepStrictEqual(cursorDeleted, oldest);
        // Fewer than ten entries: all of them go.
        cacheDeleted.length = 0;
        cursorDeleted.length = 0;
        const few = fakeIndexedDB({ stores: ["LRUMetadata"], transaction: transaction(rows.slice(0, 3)) });
        await load({ indexedDB: few.indexedDB, navigator: { storage: { estimate: async () => ({ usage: 9999 }) } } }).context.enforceLRUQuota(cache);
        assert.strictEqual(cacheDeleted.length, 3);
        // A database that will not open is logged, not thrown.
        const broken = load({ indexedDB: fakeIndexedDB({ fail: true }).indexedDB, navigator: { storage: { estimate: async () => ({ usage: 9999 }) } } });
        assert.strictEqual(await broken.context.enforceLRUQuota(cache), undefined);
        assert.strictEqual(broken.logged.length, 1);
    },

    // [@ANCHOR: caching:js_sw_harness_previous_version_cache_exists]
    async previous_version_cache_exists() {
        const names = async (list) => (await load({ caches: { keys: async () => list } }).context.previousVersionCacheExists());
        assert.strictEqual(await names([]), false);
        assert.strictEqual(await names(["odoo-assets-cache-new"]), false);
        assert.strictEqual(await names(["ham-shack-offline-1", "other"]), false);
        assert.strictEqual(await names(["ham-shack-offline-1", "odoo-assets-cache-old"]), true);
        // The activate handler tells open pages about a new version only on an update, not on the first install.
        const activate = async (list) => {
            const posted = [];
            const { listeners, self } = load({ caches: { keys: async () => list, delete: async () => true } });
            self.clients = { claim: async () => {}, matchAll: async () => [{ postMessage: (message) => posted.push(message) }] };
            let finished;
            listeners.activate({ waitUntil: (promise) => { finished = promise; } });
            await finished;
            return posted;
        };
        assert.deepStrictEqual(plain(await activate(["odoo-assets-cache-old"])), [{ type: "NEW_VERSION_INSTALLED" }]);
        assert.deepStrictEqual(await activate([]), []);
    },
};

const run = tests[process.argv[3]];
if (!run) {
    console.error(`no test named ${process.argv[3]}`);
    process.exit(2);
}
run().then(
    () => process.exit(0),
    (error) => {
        console.error(error && error.stack ? error.stack : error);
        process.exit(1);
    }
);
