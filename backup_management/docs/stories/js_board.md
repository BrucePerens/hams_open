# `backup_management/static/src/components/board/board.js`

The backup board lists each backup configuration with its latest job, and lets an operator start a backup or sync snapshots. Tests: `static/tests/board.test.js`.

The file defines one OWL component, `BackupBoard`, registered as the client action `backup_management.board` and
rendered by the QWeb template `backup_management.BackupBoardTemplate` (`board.xml`). Its state is
`this.state = useState({configs: [], isLoading: true})`: `configs` is the list returned by the server method
`backup.config.get_board_data` (one dict per configuration, see the `COMM_backup_board_data` entry in
`backup_management/README.md`), and `isLoading` is true until the first load finishes, which makes the template show
placeholder cards. All server calls use the OWL `orm` service, so they run as the logged-in user and respect record
rules; no data is cached between mounts.

## `setup()` [@ANCHOR: backup_management:js_board_setup]

Called once by OWL when the component is created; no arguments, no return value. It obtains the `orm` and `website`
services with `useService`, creates the reactive `state` (`configs: []`, `isLoading: true`) and registers an
`onMounted` hook that awaits `fetchData()`, so the board loads its data after the first render and the
placeholders show meanwhile. It also registers an empty `onWillUnmount` hook (there is nothing to clean up: no timers
or listeners are created). It does not poll; the list refreshes only after `triggerBackup`, `syncSnapshots` or the
Refresh button, which is bound to `fetchData`. Test: `static/tests/board.test.js` ("setup loads the configurations when
the board is mounted").

## `triggerBackup(configId)` [@ANCHOR: backup_management:js_board_trigger_backup]

| Argument | Type | Meaning |
|---|---|---|
| `configId` | number | The `id` of the `backup.config` record, taken from the card the operator clicked. |

Asynchronous; returns a promise that resolves to nothing. It awaits `orm.call("backup.config", "action_trigger_backup",
[configId])`, which queues a backup job for the backup worker (it does not wait for the backup to finish), then awaits
`fetchData()` so the card shows the new `pending` job and the Backup button turns into a spinner and is disabled while the
latest job is `pending` or `processing`. Errors from the server (for example `AccessError` for a user who is not a Backup
Administrator, or an unsafe target path) reject the promise and are shown by Odoo's standard RPC error dialog; the
component does not catch them, and the board is not reloaded in that case. Calling it twice queues two backups. Test:
`static/tests/board.test.js` ("triggerBackup starts a backup for the configuration and reloads the board").

## `syncSnapshots(configId)` [@ANCHOR: backup_management:js_board_sync_snapshots]

| Argument | Type | Meaning |
|---|---|---|
| `configId` | number | The `id` of the `backup.config` record to sync. |

Asynchronous; resolves to nothing. It awaits `orm.call("backup.config", "action_sync_snapshots", [configId])`, which
queues a `sync_snapshots` job (the worker runs `kopia snapshot list` or `pgbackrest info` and the results are stored
as `backup.snapshot` rows), then awaits `fetchData()`. The returned board data is the state when the call finished, so
the new snapshot list usually appears only after the worker is done and the operator refreshes. Errors reject the
promise as for `triggerBackup`. The Sync button is disabled while the configuration's latest job is `pending` or
`processing`. Test: `static/tests/board.test.js` ("syncSnapshots syncs the configuration's snapshots and reloads the
board").

## `fetchData()` [@ANCHOR: backup_management:js_board_fetch_data]

Asynchronous; no arguments; resolves to nothing. It builds an RPC `context` that is empty, or holds `website_id` set to
`this.website.currentWebsite.id` when the page is on a website, so the server limits the list to configurations of that
website or with no website (isolation by website). It sets `this.state.configs` to the result of
`orm.call("backup.config", "get_board_data", [], {context})` and then sets `this.state.isLoading = false`. If the RPC
rejects, `isLoading` stays true and the error dialog shows, so the placeholders remain on screen (known gap; to-do: reset
`isLoading` in a `finally`). It is called by `setup` on mount, after each button action and by the Refresh button
(`o_tour_refresh_board_btn`), which is disabled while `isLoading` is true. Test: `static/tests/board.test.js`
("fetchData asks for the current website's configurations, or all of them when there is no website").
