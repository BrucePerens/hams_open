/** @odoo-module **/
/* Copyright © HAMS project. AGPL-3.0-or-later. */

// OfflineStore had zero test coverage before this -- the 2026-09-11 Service Worker reliability
// review found a real durability bug here (saveLog() used to resolve on the individual IDB
// request's own onsuccess rather than transaction.oncomplete, so a QuotaExceededError abort
// after that onsuccess fired told the caller a QSO log was safely queued when it had actually
// been rolled back and lost -- see night_shift_todo.md's own writeup). Forcing a real
// QuotaExceededError reliably inside a hoot browser test is impractical (it depends on the
// browser's actual storage quota, not something this suite can control), so this covers the
// real, always-exercised round-trip behavior instead: save, list, and delete against a real
// IndexedDB in a real browser (hoot runs in real Chrome, not a mock DOM) -- proving the
// oncomplete-based Promise settlement actually resolves/rejects correctly on the happy path,
// which is the one thing that had never been directly exercised at all.
import { afterEach, describe, expect, test } from "@odoo/hoot";
import { OfflineStore } from "@zero_sudo/js/offline_store";

// A fresh, unique-per-test database name avoids any cross-test IndexedDB state bleeding between
// runs in the same browser profile -- simpler and more robust than trying to clear/reset a
// shared database between tests.
let dbCounter = 0;
function makeStore() {
    dbCounter += 1;
    return new OfflineStore(`OfflineStoreTestDB_${dbCounter}`, "test_logs");
}

const openDatabases = [];

afterEach(() => {
    // Best-effort cleanup: delete every database this suite created so a long-running browser
    // session (or a re-run of this same suite) doesn't accumulate one IndexedDB database per
    // test run forever. Not awaited -- indexedDB.deleteDatabase() can block on a lingering open
    // connection, and a cleanup failure here must never fail the test that already passed.
    while (openDatabases.length) {
        const name = openDatabases.pop();
        try {
            indexedDB.deleteDatabase(name);
        } catch {
            // Best-effort only, see comment above.
        }
    }
});

describe("offline_store", () => {
    describe.current.tags("zero_sudo_offline_store");

    test("saveLog() resolves with a real uuid, and getPendingLogs() finds it", async () => {
        const store = makeStore();
        openDatabases.push(store.dbName);

        const uuid = await store.saveLog({ callsign: "W1AW", mode: "FT8" });
        expect(typeof uuid).toBe("string");
        expect(uuid.length).toBeGreaterThan(0);

        const pending = await store.getPendingLogs();
        expect(pending.length).toBe(1);
        expect(pending[0].uuid).toBe(uuid);
        expect(pending[0].data.callsign).toBe("W1AW");
        expect(pending[0].data.mode).toBe("FT8");
    });

    test("multiple saveLog() calls all appear in getPendingLogs()", async () => {
        const store = makeStore();
        openDatabases.push(store.dbName);

        const uuid1 = await store.saveLog({ callsign: "W1AW" });
        const uuid2 = await store.saveLog({ callsign: "K6BP" });

        const pending = await store.getPendingLogs();
        const uuids = pending.map((log) => log.uuid);
        expect(pending.length).toBe(2);
        expect(uuids.includes(uuid1)).toBe(true);
        expect(uuids.includes(uuid2)).toBe(true);
    });

    test("deleteLog() removes exactly the targeted entry, leaving the rest", async () => {
        const store = makeStore();
        openDatabases.push(store.dbName);

        const uuidToDelete = await store.saveLog({ callsign: "DELETE_ME" });
        const uuidToKeep = await store.saveLog({ callsign: "KEEP_ME" });

        await store.deleteLog(uuidToDelete);

        const pending = await store.getPendingLogs();
        const uuids = pending.map((log) => log.uuid);
        expect(pending.length).toBe(1);
        expect(uuids.includes(uuidToKeep)).toBe(true);
        expect(uuids.includes(uuidToDelete)).toBe(false);
    });

    test("getPendingLogs() on a brand-new store returns an empty list, not an error", async () => {
        const store = makeStore();
        openDatabases.push(store.dbName);

        const pending = await store.getPendingLogs();
        expect(pending.length).toBe(0);
    });
});
