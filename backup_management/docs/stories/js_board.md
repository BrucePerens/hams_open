# `backup_management/static/src/components/board/board.js`

The backup board lists each backup configuration with its latest job, and lets an operator start a backup or sync snapshots. Tests: `static/tests/board.test.js`.

- `setup()` loads the configurations when the board is mounted. [@ANCHOR: backup_management:js_board_setup]
- `triggerBackup()` starts a backup for one configuration and reloads the board. [@ANCHOR: backup_management:js_board_trigger_backup]
- `syncSnapshots()` syncs one configuration's snapshots from the repository and reloads the board. [@ANCHOR: backup_management:js_board_sync_snapshots]
- `fetchData()` loads the configurations for the website the page is on, or all of them when it is on none, and ends the loading state. [@ANCHOR: backup_management:js_board_fetch_data]
