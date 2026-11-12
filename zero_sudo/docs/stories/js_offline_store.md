# `zero_sudo/static/src/js/offline_store.js`

The offline queue keeps what a page could not send (a QSO, an ICS form) in the browser's IndexedDB until it can be sent. It is a
shared, generic class: it began in the ham shack module with fixed database names and was moved here and parameterised so
`ics_forms` and any other page can queue locally without depending on the ham shack. Each page uses its own instance. Tests:
`static/tests/offline_store.test.js`. A stored entry is `{uuid, timestamp, data}`: `uuid` is the key, `timestamp` is indexed,
`data` is whatever the caller passed (it must be structured-cloneable). Every method is async and works on one IndexedDB
database in the browser; nothing is sent to the server by this file, and nothing is encrypted, so do not queue a secret.

### `new OfflineStore(dbName = 'HamShackOfflineDB', storeName = 'offline_logs')` `[@ANCHOR: zero_sudo:js_offline_store_constructor]`

| Argument | Type | Meaning |
|---|---|---|
| `dbName` | string | the IndexedDB database name |
| `storeName` | string | the object store inside it |

Records the two names, sets the schema version to 1 and leaves the connection closed (`db` is `null`). Nothing is opened until
the first call. The defaults are the ham shack's original names, so data already stored there is still found. Two instances with
the same names share one database. No errors.

### `init()` `[@ANCHOR: zero_sudo:js_offline_store_init]`

No arguments. Returns a promise that resolves (with no value) once the database is open and `this.db` is set. On the first open it
creates the object store keyed by `uuid`, with a non-unique `timestamp` index, when that store does not yet exist; an existing
store is left alone. Rejects with the browser's error when it refuses to open the database (private mode, blocked storage),
after logging it to the console. The other methods call it themselves when `db` is not set, so calling it first is optional.

### `saveLog(logData)` `[@ANCHOR: zero_sudo:js_offline_store_save_log]`

| Argument | Type | Meaning |
|---|---|---|
| `logData` | any structured-cloneable value | the entry's payload, kept as `data` |

Returns a promise for the new entry's `uuid` (from `crypto.randomUUID()`, stamped with `Date.now()`). It resolves only when the
transaction has committed (`oncomplete`), not when the request succeeds, because a quota error aborts the whole transaction after the
request's own success has fired and the entry would be lost. It rejects with the transaction's error on `onerror` and on
`onabort` (so a rolled-back write never leaves the promise hanging); `isStorageFullError` recognises the quota case. After the
commit, when the browser has a service worker and `SyncManager`, it asks the worker to register the `sync-offline-logs`
background sync; failures there are only warned, never rejected. Opens the database if needed.

### `getPendingLogs()` `[@ANCHOR: zero_sudo:js_offline_store_get_pending_logs]`

No arguments. Returns a promise for an array of every entry still queued (in key, that is uuid, order, not time order; sort by `timestamp` if order matters), in a
read-only transaction. Rejects with the request's error, or with the transaction's error (or a generic "Transaction aborted") when
it aborts, so it cannot hang. Does not remove anything; the caller deletes an entry with `deleteLog` after it has been sent.

### `deleteLog(uuid)` `[@ANCHOR: zero_sudo:js_offline_store_delete_log]`

| Argument | Type | Meaning |
|---|---|---|
| `uuid` | string | the key returned by `saveLog` |

Returns a promise that resolves (with no value) once the removal has committed, for the same durability reason as `saveLog`.
Deleting a key that is not there still resolves. Rejects on the transaction's error or abort. Opens the database if needed. Call
it only after the server has accepted the entry, or the data is lost.
