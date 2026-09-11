/** @odoo-module **/
/* Copyright © HAMS project. AGPL-3.0-or-later. */

// Shared, generic IndexedDB-backed offline queue. Originally lived in ham_shack (as the QSO
// logger's own offline_store.js) with a hardcoded 'HamShackOfflineDB'/'offline_logs' db/store
// name; moved here and parameterized so ics_forms (and any other frontend module needing the
// same "queue locally, sync when back online" pattern) can reuse it without depending on
// ham_shack, which would be architecturally wrong (ICS forms/EMCOMM logging must work with no
// radio shack installed at all). zero_sudo is a real dependency of both. Defaults match the
// original hardcoded values exactly, so ham_shack's own existing IndexedDB database/data is
// unaffected by the move.
export class OfflineStore {
    constructor(dbName = 'HamShackOfflineDB', storeName = 'offline_logs') {
        this.dbName = dbName;
        this.dbVersion = 1;
        this.storeName = storeName;
        this.db = null;
    }

    async init() {
        return new Promise((resolve, reject) => {
            const request = indexedDB.open(this.dbName, this.dbVersion);

            request.onerror = (event) => {
                console.error('[OfflineStore] Database error:', event.target.error);
                reject(event.target.error);
            };

            request.onsuccess = (event) => {
                this.db = event.target.result;
                resolve();
            };

            request.onupgradeneeded = (event) => {
                const db = event.target.result;
                if (!db.objectStoreNames.contains(this.storeName)) {
                    // Create an object store for logs with a UUID key
                    const store = db.createObjectStore(this.storeName, { keyPath: 'uuid' });
                    store.createIndex('timestamp', 'timestamp', { unique: false });
                }
            };
        });
    }

    async saveLog(logData) {
        if (!this.db) await this.init();

        const logEntry = {
            uuid: crypto.randomUUID(),
            timestamp: Date.now(),
            data: logData
        };

        return new Promise((resolve, reject) => {
            const transaction = this.db.transaction([this.storeName], 'readwrite');
            const store = transaction.objectStore(this.storeName);
            store.add(logEntry);

            // Resolve only on transaction.oncomplete, not the individual
            // request's onsuccess: IndexedDB only guarantees durability at
            // commit. A quota-exceeded error aborts the whole transaction
            // AFTER the request's own onsuccess has already fired, so
            // resolving there would tell the caller a log was saved when it
            // was actually rolled back and lost.
            transaction.oncomplete = () => {
                // Request background sync if supported. Done here (not in
                // the request's onsuccess) so we never request a sync for a
                // write that didn't actually commit.
                if ('serviceWorker' in navigator && 'SyncManager' in window) {
                    void (async () => {
                        let registration;
                        try {
                            registration = await navigator.serviceWorker.ready;
                        } catch (err) {
                            console.warn('[OfflineStore] serviceWorker.ready rejected:', err);
                            return;
                        }
                        try {
                            await registration.sync.register('sync-offline-logs');
                        } catch (err) {
                            console.warn('[OfflineStore] Background Sync registration failed:', err);
                        }
                    })();
                }
                resolve(logEntry.uuid);
            };

            transaction.onerror = () => {
                console.error('[OfflineStore] Failed to save log:', transaction.error);
                reject(transaction.error);
            };

            // A transaction can also be silently rolled back (e.g.
            // QuotaExceededError on the request) without transaction.onerror
            // firing -- it goes straight to onabort instead. Without this
            // handler, that path leaves this Promise permanently unsettled.
            transaction.onabort = () => {
                const err = transaction.error || new Error('Transaction aborted');
                console.error('[OfflineStore] Failed to save log (aborted):', err);
                reject(err);
            };
        });
    }

    async getPendingLogs() {
        if (!this.db) await this.init();

        return new Promise((resolve, reject) => {
            const transaction = this.db.transaction([this.storeName], 'readonly');
            const store = transaction.objectStore(this.storeName);
            const request = store.getAll();

            request.onsuccess = (event) => {
                resolve(event.target.result);
            };

            request.onerror = (event) => {
                reject(event.target.error);
            };

            // Same hang-bug class as saveLog()'s own transaction.onabort:
            // without this, a transaction that aborts without the
            // request's own onerror firing leaves this Promise
            // permanently unsettled.
            transaction.onabort = () => {
                reject(transaction.error || new Error('Transaction aborted'));
            };
        });
    }

    async deleteLog(uuid) {
        if (!this.db) await this.init();

        return new Promise((resolve, reject) => {
            const transaction = this.db.transaction([this.storeName], 'readwrite');
            const store = transaction.objectStore(this.storeName);
            store.delete(uuid);

            // Same reasoning as saveLog(): resolve on transaction.oncomplete
            // (real commit), not the individual request's onsuccess, and
            // handle onabort too -- otherwise a caller could be told a log
            // was removed from the offline queue when it wasn't actually
            // durable yet.
            transaction.oncomplete = () => resolve();
            transaction.onerror = () => reject(transaction.error);
            transaction.onabort = () => reject(transaction.error || new Error('Transaction aborted'));
        });
    }
}
