# `zero_sudo/static/src/js/offline_store.js`

The offline queue keeps what a page could not send (a QSO, an ICS form) in the browser's IndexedDB until it can be sent. Tests: `static/tests/offline_store.test.js`.

- `OfflineStore.constructor()` names the database and the object store it will use, defaulting to the ham shack's own names so the data already stored there is still found. [@ANCHOR: zero_sudo:js_offline_store_constructor]
- `init()` opens the database, creating the object store keyed by uuid with a timestamp index the first time, and rejects when the browser refuses to open it. [@ANCHOR: zero_sudo:js_offline_store_init]
- `saveLog()` stores one entry with a new uuid and the time, resolves with the uuid only once the write is committed, rejects when it is rolled back, and asks the service worker for a background sync when the browser has one. [@ANCHOR: zero_sudo:js_offline_store_save_log]
- `getPendingLogs()` returns every entry still queued, and rejects rather than hanging when the transaction aborts. [@ANCHOR: zero_sudo:js_offline_store_get_pending_logs]
- `deleteLog()` removes one entry by uuid, resolving only once the removal is committed. [@ANCHOR: zero_sudo:js_offline_store_delete_log]
