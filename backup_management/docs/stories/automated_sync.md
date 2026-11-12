# Automated Backup Sync [@ANCHOR: backup_management:COMM_story_automated_sync]

The system maintains a synchronized view of offsite backup states through a polling mechanism.

1. **Cron Trigger**: A global cron job (`cron_sync_all_backups`) runs hourly, and a sibling daily cron (`cron_trigger_scheduled_backups`) actually CREATES the day's backup for every `backup.config` (running `pgbackrest backup`/`kopia snapshot create`) -- the sync cron above only ever lists/reconciles existing snapshots, it never creates one, so without this second cron a deployment would silently stop taking new backups while still showing healthy-looking sync activity.
2. **Task Offloading**: For each configuration, a sync task is pushed to RabbitMQ, where the backup worker daemon consumes it from the `backup_tasks` queue (see the [module README](../../README.md#daemon-architecture-daemonmainpy)); Odoo itself does not run the engine.
3. **Engine Execution**:
   - For **Kopia**: It executes `kopia snapshot list --json` and parses the output (`COMM_backup_sync_kopia`).

   - For **pgBackRest**: It executes `pgbackrest info --output=json` and parses the output (`COMM_backup_sync_pgbackrest`).

4. **Data Ingestion**: The worker returns JSON results to Odoo, which updates the `backup.snapshot` records. This is optimized using a Postgres stored procedure to ensure atomic, single-roundtrip bulk updates.

5. **Dashboard Update**: The aggregated data is made available for the Backup Dashboard (the NOC, network operations center, view of the latest snapshot per configuration).

## Entries

### `cron_sync_all_backups(self)` [@ANCHOR: backup_management:COMM_cron_sync_all_backups]

The hourly sync cron (`data/cron.xml`, record `cron_sync_backups`, "Backup Management: Sync Snapshots", runs as
`user_backup_service_internal`, code `model.cron_sync_all_backups()`). It is `@api.private @api.model` and takes no
arguments; it returns nothing. As the service account it loads up to 1000 `backup.config` records and reads each one's
newest snapshot start time with one SQL `GROUP BY` query. Per configuration it then: calls `action_sync_snapshots()`
(queues a `sync_snapshots` job); if the newest snapshot is older than 26 hours (24 hours of cron jitter plus margin)
calls `report_backup_failure("Stale Backup Alert: ...")`; reports each message returned by `_backup_job_problems` (the
newest three jobs all failed, or no job succeeded in 26 hours); and, if `restore_drill_script` is set and
`last_drill_time` is empty or older than 7 days, calls `_execute_restore_drill()`. A configuration with no snapshot at all
is not alerted on as stale. Errors from a single configuration are not caught, so one failure stops the rest of that run;
the next hour tries again. It does not run the engine; Odoo only queues. Side effects: jobs, RabbitMQ messages after
commit, pager incidents, chatter posts. Tests: `tests/test_backup.py::test_04_cron_trigger`.

### `cron_trigger_scheduled_backups(self)` [@ANCHOR: backup_management:COMM_cron_trigger_scheduled_backups]

The daily cron (record `cron_trigger_scheduled_backups`, interval 1 day, same service user) that actually CREATES backups.
`@api.private @api.model`, no arguments, no return value. It searches up to 1000 `backup.config` records as the service
account and calls `action_trigger_backup()` on all of them, which queues one `kopia snapshot create` or
`pgbackrest backup --type=full` job each. The hourly sync only lists existing snapshots, so without this cron backups
would silently stop after manual use ends while the dashboard still showed healthy sync activity. The daily interval
matches the `keep_daily` retention field. Failures to queue are handled per job (`_mark_dispatch_failed`), and a
configuration that cannot be queued raises (for example an invalid local `target_path`), which stops the loop in
`_publish_to_worker`; this is a to-do to isolate per configuration. Test:
`tests/test_backup.py::test_05_cron_trigger_scheduled_backups_creates_a_real_job_for_every_config`.

### Kopia sync [@ANCHOR: backup_management:COMM_backup_sync_kopia]

Kopia half of `backup.config.action_sync_snapshots` (the tag is a comment in that method; the tags
`UX_BACKUP_SYNC` and `COMM_backup_sync_pgbackrest` sit beside it). The method is `@public_rpc`, takes only `self` and
returns `True`. For each configuration it calls `_publish_to_worker("sync_snapshots")`, which creates a `pending`
`backup.job` and queues a message. In the daemon (`daemon/main.py`, `execute_job`) a `sync_snapshots` job whose
`config_engine` is `kopia` runs `["kopia", "snapshot", "list", "--json"]`. On exit code 0 the daemon trims the exit
trailer from the captured log, extracts the first JSON array or object with a regex, parses it and calls
`backup.config.action_process_snapshot_data(data, engine="kopia")`. A parse failure is reported with
`report_backup_failure("Sync Parse Error: ...")` and the job still ends `done`. For Kopia, each list item
with an `id` becomes `{snapshot_id: id, start_time: startTime[:19] with T replaced by a space, size_bytes:
summary.totalBytes, status: "completed"}`. Access: Backup Administrators or the service account, else `AccessError`.
Test: `tests/test_backup.py::test_01b_sync_kopia_triggered`.

### pgBackRest sync [@ANCHOR: backup_management:COMM_backup_sync_pgbackrest]

The pgBackRest half of the same flow. The daemon runs `["pgbackrest", "info", "--stanza=<target_path>", "--output=json"]`
plus `--repo=<n>` when the configuration names a repository, or the S3/B2 repository arguments for
`storage_type` s3 or b2. `info` is read-only and works as the unprivileged worker. The output is a list of stanzas;
`_process_snapshot_data` walks each stanza's `backup` list, using `label` as `snapshot_id`, `timestamp.start` (Unix
seconds, converted to a UTC `YYYY-MM-DD HH:MM:SS` string; an out-of-range value becomes no time) as `start_time`, and
`info.size` as `size_bytes`, with status `completed`. Differences from Kopia: labels come from pgBackRest
(`20261101-020001F`), sizes are the database size, not the repository delta. Test:
`tests/test_backup.py::test_02_sync_pgbackrest_triggered` (queues a pending job for a pgBackRest configuration).

### `upsert_backup_snapshots` stored procedure [@ANCHOR: backup_management:COMM_upsert_snapshots_procedure]

PL/pgSQL function created in `models/sql_views.py` `init()` (inside a savepoint, so a reinstall replaces it):
`upsert_backup_snapshots(p_config_id INTEGER, p_website_id INTEGER, p_company_id INTEGER, p_snapshots JSONB,
p_user_id INTEGER) RETURNS TABLE(out_snapshot_id VARCHAR)`.

| Argument | Meaning |
|---|---|
| `p_config_id` | The owning `backup.config` id. |
| `p_website_id`, `p_company_id` | Copied from the configuration; may be NULL. |
| `p_snapshots` | JSON array of objects with `snapshot_id`, `start_time` (`YYYY-MM-DD HH:MM:SS`, UTC), `size_bytes`, `status`. |
| `p_user_id` | Written to `create_uid` and `write_uid`. |

It inserts one `backup_snapshot` row per array element, setting `name` to the snapshot id (a raw INSERT bypasses the ORM
default, and rows used to read back with no name), `create_date` and `write_date` to `NOW()`, and ignores rows that
already exist (`ON CONFLICT (config_id, snapshot_id) DO NOTHING`, matching the model's unique constraint). It returns
only the ids that were newly inserted, which `_process_snapshot_data` uses so that the minimum-size anomaly check judges
new snapshots only and does not re-alert. Runs inside the caller's transaction; existing rows are never updated, so a
snapshot whose size changes later is not refreshed (known limit). Raw SQL: all values are bound parameters or JSON, so
the snapshot text cannot inject SQL. Test: `tests/test_backup.py::test_01b_sync_kopia_triggered`.

### Single round-trip bulk ingest [@ANCHOR: backup_management:COMM_upsert_snapshots_roundtrip_optimization]

The Python half of the previous entry: the comment marks `backup.config._process_snapshot_data(self, data, engine)`.
Public wrapper `action_process_snapshot_data(data, engine)` (`@public_rpc`, `ensure_one`) exists because Odoo refuses
RPC calls to method names that start with an underscore, and the worker calls it over JSON-2 after each successful
`sync_snapshots` job. `data` is the engine's parsed JSON (a list of Kopia snapshots, or a list of pgBackRest stanzas);
`engine` is `"kopia"` or `"pgbackrest"`; any other value produces an empty list and nothing is stored. The method builds
the whole snapshot list in Python, then runs ONE statement `SELECT upsert_backup_snapshots(config, website, company,
<json>, uid)` rather than a search and create per snapshot, collects the new ids with `fetchall()`, and then, when
`minimum_size_mb > 0`, calls `report_backup_failure("Snapshot Anomaly: ...")` for each new snapshot below the minimum.
Returns nothing. Cost: constant database round trips regardless of snapshot count. Test:
`tests/test_backup.py::test_01b_sync_kopia_triggered`.

### `get_board_data(self)` [@ANCHOR: backup_management:COMM_backup_board_data]

The data source of the Backup Dashboard (`static/src/components/board/board.js` calls it with
`orm.call("backup.config", "get_board_data", [], {context})`). `@public_rpc @api.model`, no arguments except the
context key `website_id`, which limits the list to configurations with that website or none. Returns a list of dicts,
one per configuration (at most 10000): `id`, `name`, `engine`, `target_path` plus `latest_snapshot` (the row from
`backup.latest.snapshot.view`: `config_id`, `snapshot_id`, `start_time`, `size_bytes`, `status`, or `False`),
`is_stale` (True when there is no snapshot or the newest started more than 26 hours ago) and `latest_job` (a
dict of `id`, `config_id`, `state`, `job_type`, `create_date` for the newest job, or `False`). It uses `search_read`, two
queries for snapshots and jobs (one `DISTINCT ON (config_id)` SQL), so it does not scale with the number of
configurations in queries. Read-only; record rules decide which configurations the caller sees. Returns `[]` when there
are none. Test: `tests/test_backup.py::test_09_board_data_rpc`.

### Knowledge documentation bootstrap [@ANCHOR: backup_management:COMM_backup_doc_injection]

The `knowledge_docs` key of `__manifest__.py`: a list with one entry `{"name": "Backup Management", "path":
"data/documentation.html", "icon", "category": "workspace"}`. `post_init_hook` in `hooks.py` runs, as the backup
service account, `env["ir.module.module"]._bootstrap_knowledge_docs()`, which creates the Knowledge article from that
HTML when the module is installed, so the user guide exists without anyone writing it into the database by hand. The
hook runs once at install; changing `documentation.html` afterwards does not update the article on existing
databases (unknown whether a module upgrade refreshes it; to-do: check `_bootstrap_knowledge_docs`). Test:
`tests/test_backup.py::test_12_documentation_installation` (asserts the article exists and its body contains "Backup Management User Guide").

### `action_test_connection(self)` [@ANCHOR: backup_management:COMM_action_test_connection]

`@public_rpc` button on the configuration form, no arguments besides `self`. For each configuration it calls
`action_sync_snapshots()` (so a connection test is a snapshot listing: `kopia snapshot list` or `pgbackrest info`)
and returns the client action `display_notification` with the title "Connection Test Triggered", the message that the
test was offloaded to the background worker, and `sticky: False`. It does not wait for the result and does not itself
report success or failure: the outcome appears in the job log and, for a failed sync, in the failure alerts. Errors are
those of `_publish_to_worker` (`AccessError` for other users, `UserError` for a bad local path). Side effect: one
`pending` job and one queued message per configuration. Test: `tests/test_backup.py::test_09_board_data_rpc`.

### `action_view_latest_job(self)` [@ANCHOR: backup_management:COMM_action_view_latest_job]

`@public_rpc`, requires exactly one configuration (`ensure_one`). It searches `backup.job` for the configuration with
`limit=1` (the model's default order, newest first) and returns an `ir.actions.act_window` dict for the job form
(`res_model=backup.job`, `res_id`, `view_mode=form`, `target=current`). Raises `UserError("No jobs found for this
configuration.")` when there are none. Read-only; the job record rules apply to the caller. The job form shows the live
output log that the daemon appends with `append_log`. Test: `tests/test_backup.py::test_09_board_data_rpc`.

### `_auto_refresh_status(self)` [@ANCHOR: backup_management:COMM_auto_refresh_status]

Cleanup of abandoned jobs, on `backup.job`. Run every minute by cron `ir_cron_auto_refresh_backup_jobs`
(`views/backup_job_views.xml`, as the service account, code `model._auto_refresh_status()`); takes no arguments and
returns nothing. First it calls `_fail_stale_pending_jobs()` (jobs still `pending` after `PENDING_TIMEOUT_HOURS` = 6
are marked failed with an explanation that they were never picked up); then it finds up to 1000 jobs in `processing`
whose `write_date` is older than 2 hours and marks them `failed`, appending "[SYSTEM] Job timed out after 2 hours of
inactivity." to the log. A job that keeps receiving log chunks keeps refreshing `write_date`, so a long, chatty backup is
not cut off, but a silent engine that runs more than two hours is. A separate `action_refresh_status` button method
is a no-op that only lets the UI reload. Tests: `tests/test_backup_job.py`
(`test_stale_pending_job_is_failed_with_a_reason_and_fresh_ones_are_not`) and `tests/test_backup.py::test_09_board_data_rpc`.
