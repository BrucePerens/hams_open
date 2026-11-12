# Journey: Backup Configuration and First Sync [@ANCHOR: backup_management:COMM_journey_backup_config_sync]

This journey walks through the initial setup of a backup target and its first metadata synchronization.

1. **Create Configuration**: The administrator navigates to **Ham Admin > Backups > Configurations** and creates a new record (exercised by the browser tour, see Entries).
2. **Set Parameters**: They choose the engine (Kopia), set the target path (e.g., `/var/lib/odoo/backups/repo`), and provide the encryption password.
3. **Save**: Upon saving, the system validates the paths and encrypts the password.
4. **Trigger Sync**: The admin clicks "Sync Snapshots".
5. **Execution**: Odoo queues the engine-specific list command for the backup worker (see Entries).
6. **Result**: The "Snapshots" tab populates with the list of available backups retrieved from the engine.

## Entries

### Backup dashboard tour [@ANCHOR: backup_management:COMM_backup_dashboard_tour]

The browser tour `backup_dashboard_tour` (`static/src/tests/tours/backup_dashboard.js`, run by
`tests/test_tour.py::test_backup_dashboard_tour` as the admin user) walks steps 1 to 3 of this journey: it opens the Backup
Management app and the Backups submenu, opens Configurations, creates a record named "Test Kopia Tour" with engine Kopia
and target path `/var/lib/odoo/backups/tour_repo`, and saves it. It then clicks "Backup Now", opens the Backup Dashboard
and presses Refresh. It sets no encryption password and does not click "Sync Snapshots" (step 4); the Python side of the
test calls `action_sync_snapshots()` on a pgBackRest configuration and asserts a `pending` `backup.job` exists. The
tour only creates test data and is rolled back with the test database. The JS file spells the tag
`COMM_backup_dashboard_tour`, without the module prefix.

### Kopia sync step [@ANCHOR: backup_management:COMM_backup_sync_kopia]

Step 5 of the journey. Odoo itself does not execute anything: `backup.config.action_sync_snapshots()` creates a `pending`
`backup.job` of type `kopia`, queues it for the backup worker, and the worker runs `kopia snapshot list --json`. The
resulting list is sent back through `action_process_snapshot_data(data, "kopia")` and stored by the
`upsert_backup_snapshots` SQL function, so the Snapshots tab fills only after the worker has finished, not at the moment of the click; a page reload shows it. Errors: `AccessError` for a user who is not a Backup
Administrator, `UserError` for an invalid local target path; a Kopia failure shows in the job log and as a pager
incident. Full entry: `docs/stories/automated_sync.md`. Test: `tests/test_backup.py::test_01b_sync_kopia_triggered`.
