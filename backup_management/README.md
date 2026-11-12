import: `hams_open.backup_management`

# Backup Management Module

This module provides a unified interface for managing system backups using Kopia (for files) and pgBackRest (for PostgreSQL databases). It is designed to work in multi-website and multi-company environments.

## Features

- **Multi-Engine Support:** Manage both Kopia and pgBackRest from a single dashboard.
- **Asynchronous Execution:** Backup and restore jobs are offloaded to a background worker via RabbitMQ to prevent UI blocking. Each request first creates a `backup.job` record in state `pending`; the RabbitMQ message is sent only after the database transaction commits. If that send fails, the job is marked `failed` with a note that it was not run and must be triggered again. A job that is still `pending` six hours after it was created (its queue message was lost, or the worker could not reach Odoo to start it) is marked `failed` with a note that it was not run, by the same every-minute cron that fails `processing` jobs idle for two hours (`backup.job._auto_refresh_status`).
- **Live Job Status:** The worker daemon writes each job's state back to Odoo as it runs (`processing`, then `done` or `failed`) and appends its output to the job log. A cron job ("Backup Job Auto-Refresh", every minute) runs `backup.job._auto_refresh_status()`, which marks a job that has sat in `processing` for more than 2 hours without being written to as `failed`.
- **Performance Optimized:** Bulk snapshot synchronization uses a PostgreSQL function (`upsert_backup_snapshots`) that inserts every snapshot from one sync in a single statement, skipping snapshots already recorded, and returns only the new ones.
- **Multi-Tenant Aware:** Backups and snapshots are isolated by website and company.
- **Scheduled Backups:** A daily cron job ("Backup Management: Scheduled Backup Run") queues a backup for every configuration. A separate hourly cron job ("Backup Management: Sync Snapshots") refreshes the snapshot list, raises stale-backup alerts and queues restore drills (see below); it never creates a backup.
- **Automated Retention:** Configure daily, weekly, and monthly retention policies. For Kopia they are pushed to the repository (`kopia policy set`) only when an administrator clicks **Apply Policies**. For pgBackRest only **Keep Daily** is used, as `--repo1-retention-full` on each backup; the weekly and monthly values are ignored.
- **Health Monitoring:** Automated stale backup detection and size anomaly alerts via PagerDuty (this project's `pager_duty` Odoo module, not the commercial service): `report_backup_failure()` opens a critical `pager.incident` and posts the message on the configuration's chatter. The hourly sync cron alerts when a configuration's newest recorded snapshot started more than 26 hours ago (a configuration with no recorded snapshot at all is not alerted on), and each newly recorded snapshot smaller than the configuration's **Minimum Size (MB)** raises an anomaly alert (only when that field is above 0).
- **Restore Drills:** Automated restore tests to verify backup integrity. If a configuration names a **Restore Drill Script**, the hourly sync cron queues it whenever the last successful drill is more than 7 days old. The worker runs the script only if it is an executable `.py` file under `BACKUP_WORKER_SCRIPTS_DIR` (default `/opt/hams/daemons/backup_worker/scripts`), and records **Last Successful Drill** when it exits with code 0.
- **Zero-Sudo Security:** All background operations and API calls use dedicated service accounts for maximum security. "Zero-Sudo" is this codebase's rule (enforced by the `zero_sudo` module) that Odoo's `.sudo()` is never used; privileged steps instead run as a named service account, here `backup_service_internal`, resolved with `zero_sudo.security.utils._get_service_uid()`.

## Technical Specification

### 1. Automated Volume Synchronization
Copies the snapshot list reported by Kopia (`kopia snapshot list`) or pgBackRest (`pgbackrest info`) into `backup.snapshot`, from the hourly cron job or the **Sync Snapshots** button.
These are the anchors of the sync and retention features. Longer treatment of the sync flow is in
[automated_sync](docs/stories/automated_sync.md); each entry here stands on its own.

### `action_sync_snapshots` Kopia path [@ANCHOR: backup_management:COMM_backup_sync_kopia]

`backup.config.action_sync_snapshots()` (`@public_rpc`, takes only `self`, returns `True`) queues a `sync_snapshots`
job per configuration through `_publish_to_worker`. For a Kopia configuration the worker runs `kopia snapshot list
--json`, and on success sends the parsed list to `action_process_snapshot_data(data, "kopia")`. Each item with an `id`
becomes a snapshot row: id, start time (`startTime` cut to seconds), size (`summary.totalBytes`), status `completed`.
Errors: `AccessError` unless the caller is a Backup Administrator or the service account; a parse failure is reported as
"Sync Parse Error" through `report_backup_failure`. Odoo never runs Kopia itself. Test: `tests/test_backup.py::test_01b_sync_kopia_triggered`.

### `action_sync_snapshots` pgBackRest path [@ANCHOR: backup_management:COMM_backup_sync_pgbackrest]

For a pgBackRest configuration the same method queues a job whose worker command is `pgbackrest info
--stanza=<target_path> --output=json` (with `--repo=<n>` or S3/B2 repository flags as configured). `info` is read-only,
so it runs in the unprivileged worker. `action_process_snapshot_data(data, "pgbackrest")` walks every stanza's `backup`
list: `label` is the snapshot id, `timestamp.start` (Unix seconds) becomes a UTC start time, `info.size` the size. An
out-of-range timestamp gives no start time rather than an error. Test: `tests/test_backup.py::test_02_sync_pgbackrest_triggered`.

### `cron_sync_all_backups` [@ANCHOR: backup_management:COMM_cron_sync_all_backups]

The hourly cron "Backup Management: Sync Snapshots" (`data/cron.xml`), running as the backup service account; no
arguments, no return value. For up to 1000 configurations it queues a sync job, reports a stale-backup alert when the
newest recorded snapshot started more than 26 hours ago (none recorded: no alert), reports the messages from
`_backup_job_problems` (three failed jobs in a row, or no successful job in 26 hours), and queues the restore drill when
the last drill is older than 7 days. It only queues and alerts; it never creates a backup. An exception for one
configuration ends that run; the next hour retries. Test: `tests/test_backup.py::test_04_cron_trigger`.

### `upsert_backup_snapshots` procedure [@ANCHOR: backup_management:COMM_upsert_snapshots_procedure]

PL/pgSQL function created by `models/sql_views.py` `init()`: `upsert_backup_snapshots(p_config_id, p_website_id,
p_company_id, p_snapshots JSONB, p_user_id) RETURNS TABLE(out_snapshot_id VARCHAR)`. It inserts every snapshot of one
sync in a single statement, setting `name` to the snapshot id, and skips rows that already exist
(`ON CONFLICT (config_id, snapshot_id) DO NOTHING`); existing rows are never updated. It returns only the newly inserted
ids, which the caller uses to apply the minimum-size anomaly check to new snapshots only. Bound parameters only, so no
SQL injection from snapshot text. Full table and notes in [automated_sync](docs/stories/automated_sync.md).

### `action_apply_policies(self)` [@ANCHOR: backup_management:COMM_backup_apply_policies]

The **Apply Policies** button of `backup.config` (`@public_rpc`, only `self`). For each configuration whose engine is
Kopia it queues a `kopia_policy` job (`_publish_to_worker("kopia_policy")`); pgBackRest configurations are skipped,
because policies are a Kopia concept and the job would only fail against the wrong tool. The worker runs `kopia policy set
-- <target_path> --keep-latest=<keep_daily> --keep-daily --keep-weekly --keep-monthly` plus one `--add-ignore=<line>` per
non-blank line of `exclude_patterns` (defaults 7, 4, 6 when absent). It overwrites the repository's policy. Returns `True`
or, for a single record, the job form action. Errors as for `action_trigger_backup`. Test:
`tests/test_backup.py::test_08_apply_policies_triggered` and `test_08b_apply_policies_skips_non_kopia_configs`.

### `get_board_data(self)` [@ANCHOR: backup_management:COMM_backup_board_data]

Source of the Backup Dashboard (`@public_rpc @api.model`; optional context `website_id`). Returns a list of at most
10000 dicts, one per configuration visible to the caller: `id`, `name`, `engine`, `target_path`, `latest_snapshot`
(newest row of `backup.latest.snapshot.view` or `False`), `is_stale` (no snapshot, or newest older than 26 hours) and
`latest_job` (newest job's `id`, `config_id`, `state`, `job_type`, `create_date`, or `False`). Three queries however many
configurations there are. Read-only; record rules limit the rows. Test:
`tests/test_backup.py::test_09_board_data_rpc`; the component is `static/src/components/board/board.js`.

## Models

- `backup.config`: Stores the configuration for Kopia or pgBackRest, including retention policies, targets, bucket info, and storage type.
- `backup.job`: Represents an asynchronous background task (e.g., snapshot, restore, sync). Tracks state (pending, processing, done, failed) and streams output logs from the worker.
- `backup.snapshot`: Represents a completed backup snapshot. Holds metadata such as size, time, and dynamically computes the restore command.
- `backup.restore.wizard`: Transient model to guide the user (Admins only) through restoring a specific snapshot. It restores Kopia snapshots only; it refuses pgBackRest snapshots.
- `backup.latest.snapshot.view`: Read-only SQL view holding the newest snapshot of each configuration; the Backup Dashboard reads it.
- `utils.py`: Provides utility methods like path validation (`validate_backup_path`) and RabbitMQ publishing (`publish_to_rabbitmq`).

## Daemon Architecture (`daemon/main.py`)

The Backup Daemon is an independent worker running `main.py` that listens to the `backup_tasks` RabbitMQ queue.
- **Flow:** Odoo posts a JSON payload to RabbitMQ. The daemon consumes the payload and executes the requested binary (Kopia, pgBackRest, etc.) securely outside of Odoo.
- **pgBackRest Sidecar:** `pgbackrest backup` and `pgbackrest info` (used to list snapshots) are the commands the daemon does not run itself, because they need access to PostgreSQL's data directory and to `/etc/pgbackrest/pgbackrest.conf`, which the daemon's own account cannot read. The daemon writes a request file into `PGBACKREST_SPOOL_DIR`; a separate privileged sidecar (`daemon/pgbackrest_sidecar.py`) validates it, runs pgBackRest as the `postgres` user, and writes a result file back. The daemon waits up to `PGBACKREST_SIDECAR_TIMEOUT` seconds (default 3600) and fails the job if no result arrives.
- **Communication:** It connects back to Odoo via the JSON-2 API (e.g., `/json/2/backup.job/write`) using a dedicated internal service account (`backup_service_internal`, authenticated with the API key in the `ODOO_RPC_KEY` environment variable) to stream log buffers, update statuses, and report failures.
- **Failure Handling:** An error while processing one message fails only that job: the daemon marks it `failed`, appends the error to its log, calls `report_backup_failure()`, and acknowledges the message so it is not redelivered.
- **S3 and Backblaze B2 storage:** a `backup.config` of storage type S3 or B2 reaches the daemon with a bucket, an optional endpoint URL, an optional region and the access and secret keys. Three helpers turn that into engine settings:
  - `_strip_endpoint_scheme()` reduces an administrator-entered endpoint such as `https://s3.us-west-002.backblazeb2.com` to the bare host kopia's `--endpoint` flag requires (kopia rejects a scheme), and reports whether TLS must be disabled, which is true only for an explicit `http://`. [@ANCHOR: backup_management:COMM_strip_endpoint_scheme]
  - `_ensure_kopia_s3_repository()` makes sure a kopia repository is connected for the bucket, using one config file per `backup.config` so jobs for different buckets never share a connection. kopia has no connect-or-create command, so it tries `repository connect` first, which is the case on every run after the first, and runs `repository create` only if connect says the repository is not initialised. If both fail, the job fails with both errors in its log. [@ANCHOR: backup_management:COMM_ensure_kopia_s3_repository]
  - `_pgbackrest_s3_repo_args()` builds the pgBackRest repository flags (type, bucket, endpoint, region defaulting to `us-east-1`, and a path derived from the stanza name so configurations sharing a bucket do not collide). The keys are not flags, because pgBackRest refuses them on the command line; they go in `PGBACKREST_REPO1_S3_KEY` and `PGBACKREST_REPO1_S3_KEY_SECRET` environment variables, which also keeps them out of the logged command line. B2 is addressed through its S3-compatible API. [@ANCHOR: backup_management:COMM_pgbackrest_s3_repo_args]
- **Log hygiene:** `_redact_payload()` replaces the values of `kopia_password`, `secret_key` and `access_key` with `***REDACTED***` before a job payload is written to the daemon's log, because the payload Odoo puts on the queue carries those credentials in plain text. A payload that is not a dictionary is returned unchanged. [@ANCHOR: backup_management:COMM_redact_payload]
- **Snapshot ingestion:** after a `sync_snapshots` job the daemon calls `backup.config.action_process_snapshot_data(data, engine)` over JSON-2, which records the snapshots the engine listed. It is a public method on purpose: Odoo refuses to call any method starting with an underscore remotely, so the private `_process_snapshot_data()` behind it could not be reached. It works on exactly one configuration. [@ANCHOR: backup_management:COMM_action_process_snapshot_data]
- **Security:** The daemon rigidly restricts allowed commands and targets. It enforces bounds-checking, blocks malicious parameters, and ensures path isolation before making subprocess calls.

## Stuck jobs

`backup.job._fail_stale_pending_jobs()` is the Odoo-side backstop for a job whose worker never started it: a job still `pending` six hours after it was created (`PENDING_TIMEOUT_HOURS`) is marked `failed`, with a log note saying it was not run and must be triggered again. The cause is usually a lost queue message or a worker that could not reach Odoo (for example a rejected API key). A job a worker did pick up is `processing` and is left to the two-hour processing timeout. It runs from the same every-minute cron as that timeout, `_auto_refresh_status()`. [@ANCHOR: backup_management:COMM_fail_stale_pending_jobs]

## Developer & AI Guide

- **Interacting with Backups:** Never run backup binaries via `subprocess` from within Odoo models. All operations must route through RabbitMQ.
- **Triggering Jobs:** Use internal methods like `_publish_to_worker` within `backup.config` to dispatch tasks asynchronously.
- **Security Protocols:** All backend RPC calls must use service accounts (via `zero_sudo.security.utils`) to prevent unauthorized escalation. Ensure `validate_backup_path` is called on any user-provided path to prevent directory traversal and symlink attacks.

## User Guide

### Configuring a Backup
1. Navigate to **Backups > Management > Configurations**.
2. Click **New** to create a new configuration.
3. Select the **Engine** (Kopia or pgBackRest).
4. Enter the **Target / Stanza** field. For Kopia this is the path the worker passes to `kopia snapshot create`; with Local Directory storage it must lie inside one of the allowed backup directories listed in `validate_backup_path()` (`models/utils.py`). For pgBackRest it is the stanza name, which may contain only letters, digits and underscores.
5. Set retention policies (Daily/Weekly/Monthly). For Kopia, click **Apply Policies** after saving to push them to the repository.
6. Save the configuration. Saving a Kopia password or an S3/B2 secret key fails with an error unless the server has `ODOO_BACKUP_CRYPTO_KEY` or `HAMS_CRYPTO_KEY` set; those secrets are stored encrypted with that key.

### Monitoring Backups
- The **Backup Dashboard** (**Backups > Management > Backup Dashboard**) provides a high-level overview of the latest snapshots and their status. A configuration is shown as STALE / FAILED when it has no recorded snapshot or its newest recorded snapshot started more than 26 hours ago.
- **Active Jobs** (**Backups > Management > Active Jobs**) show the history and live output logs of background operations.

### Restoring a Backup
1. Go to **Backups > Management > Snapshots**.
2. Select the snapshot you wish to restore.
3. Click the **Restore Snapshot** button.
4. Fill in **Restore Directory / Stanza Target** (the field is required) and click **Restore**.

For Kopia, the typed directory is not used: the snapshot is always restored into `/var/lib/odoo/backups/restore_<job id>`, and that path is written into the restore job's log. For pgBackRest, the wizard refuses the restore with an error, because restores are not routed through the privileged sidecar; a pgBackRest restore has to be run manually.

## Cross-Module Interfaces

### Compliance Monitoring
This module does not itself detect or report cross-tenant data leakage. Tenant isolation is enforced by the record rules in `security/security.xml`, which limit Backup Administrators to configurations, snapshots and jobs that belong to one of their companies and to either no website or their own website. Related reporting elsewhere:
* **Tenant Violation Reports:** For tracking frontend moderation workflow alerts, see `[@ANCHOR: user_websites:UX_REPORT_VIOLATION]`: the public form endpoint `POST /website/report_violation` in `user_websites/controllers/main.py` (CSRF-protected, public auth). It silently discards a submission whose hidden honeypot field is filled, runs the `form.shield` checks, requires a URL and a description (description cut to 5000 characters), then creates a `content.violation.report` as the user_websites service account, recording the signed-in reporter's id from the session or the guest's email, and redirects with `report_submitted=1` or a `report_error` code. Details are in `user_websites/README.md`; this module does not handle it.
* **Automated Escalation:** Backup failures, stale backups and undersized snapshots are escalated through `report_backup_failure()` to the `pager_duty` module (see Health Monitoring above).

## External Dependencies

- `pager_duty`: Used for failure reporting and stale snapshot alerts.
- `pika`: Used for RabbitMQ integration in the daemon.
- `cryptography`: Used for Fernet encryption of the stored Kopia password and S3/B2 secret key.

## Backup health

`cron_sync_all_backups` (hourly) reports a configuration through `report_backup_failure()` (a critical `pager.incident` with source `Backup Manager: <name>`) when its newest snapshot is more than 26 hours old,
and, new, when its jobs show the worker is not working. The reports repeat each hour, so they stay one open incident whose occurrence count grows, and `pager.incident` re-pages it every six hours until someone acknowledges it
(`pager_duty:repage_if_due`).

### `[@ANCHOR: backup_management:backup_job_problems]` `backup.config._backup_job_problems(now)`

**Purpose.** Names what is wrong with a configuration's jobs, so a worker that cannot run or report jobs is noticed from the Odoo side without depending on the worker. 2026-10-27 to 11-05 (ticket 116) every job failed or was
lost for nine days while only the 26-hour snapshot alert (once) spoke.

**Call interface.**

| Name | Type | Meaning |
|---|---|---|
| `now` | `datetime` | The reference time (`fields.Datetime.now()` from the cron). |
| returns | `list[str]` | Zero, one or two messages: `Backup Jobs Failing` when the newest `CONSECUTIVE_FAILED_JOBS_ALERT` (3) jobs of any job type all have state `failed` (jobs the six-hour pending timeout marked failed count), and `No Successful Backup Job` when the configuration has a job older than 26 hours but none in state `done` created within the last 26 hours. |

Errors: none. A configuration with no job older than 26 hours is not judged, so a new one is quiet.

**Special concerns.** Two cheap indexed searches per configuration per hour. It reads only `backup.job`, so it still works while the worker is down. The caller reports each message through `report_backup_failure()`.

**See also.** `test_backup_job_problems_reports_failed_and_missing_successful_jobs`, `test_cron_sync_all_backups_reports_job_problems_every_run` in `tests/test_backup.py`.

## The worker's Odoo key (`daemon/main.py`)

The worker (`backup.worker.service`, user `odoo`) calls back to Odoo over the JSON-2 API as `backup_service_internal` with a bearer key. systemd reads that key
from `/opt/hams/etc/keys/backup_worker.env` (`ODOO_RPC_KEY=`) when the unit starts and the process keeps it in memory. A key rotation rewrites the file and the
Odoo-side key but does not restart a running worker.

### `[@ANCHOR: backup_management:reload_odoo_key]` `_reload_odoo_key()`

**Purpose.** Lets the worker pick up a rotated key without a restart. On 2026-10-27 15:34 UTC every daemon key on hams1 was rotated; the worker, started on
2026-10-22, went on sending the old key, got `401 Invalid apikey` on every call (25 a day) and the daily pgbackrest full backup to B2 (job type `pgbackrest`,
13:13 UTC) never ran again: the last one was 20261027-131329F (ticket 116, left open as "no new snapshots in 26 hours"). Called by `_json2_call` after a 401.

**Call interface.**

| Name | Type | Meaning |
|---|---|---|
| (none) | | Reads the module constant `ODOO_KEY_ENV_FILE` (default `/opt/hams/etc/keys/backup_worker.env`, overridable with the environment variable `BACKUP_WORKER_KEY_ENV_FILE`) and writes the module global `ODOO_PASS`. |
| returns | `bool` | `True` only when the file held a non-empty `ODOO_RPC_KEY` different from the key in use, which is now in use. `False` when the file is unreadable, has no such line, or holds the same key. |

Errors: none raised. An unreadable file is logged as a warning (the reason only, never the key or the path's contents).

**Special concerns.** Blocking file read of one small file; no locks (the worker handles one RabbitMQ message at a time, so nothing races the assignment).
Never logs a key. Idempotent. Only the last `ODOO_RPC_KEY=` line counts; surrounding quotes are removed.

**See also.** `test_json2_call_reloads_a_rotated_key` in `daemon/test_main.py`; `zero_sudo/daemon/json_rpc_client.py`, which does the same for every other daemon.

### `[@ANCHOR: backup_management:odoo_unavailable_error]` `OdooUnavailableError` (jobs that cannot be reported)

**Purpose.** A subclass of `OdooAPIError` that `_json2_call` raises when Odoo cannot be reached or refuses this worker's credentials: an HTTP 401 or 403 that the key reload did
not cure, an HTTP 5xx, or a connection error. It says nothing about the job. While the worker's key was refused (2026-10-27 to 11-05, ticket 116) every job it received could
neither be marked `processing` nor reported failed, and the worker acknowledged it: jobs 794 to 1019 (the hourly `sync_snapshots` and the daily pgbackrest full) were consumed and
lost without a trace. `execute_job` now puts such a job back on the queue (`basic_nack` with requeue, then waits `REQUEUE_DELAY_SECONDS`, 20 s, so the retry is not a tight loop)
when the failure and its report both failed for this reason, so the job runs once Odoo accepts the worker again. Any other error (HTTP 404 for a job that does not exist, a
parse error) is still acknowledged, so a job that can never succeed cannot loop forever. A job already acknowledged early (the pgbackrest sidecar path) is never requeued.

**Call interface.** `raise OdooUnavailableError(message)` from `_json2_call(model, method_name, svc_uid=None, **kwargs)`; `execute_job(ch, method, properties, body)` catches it.

**Special concerns.** The consumer sleeps in the callback during the back-off, which is below RabbitMQ's heartbeat window. A key that stays refused makes the message cycle every 20
seconds and log an error each time; the stale-backup alert (see "Backup health") is what pages in that case.

**See also.** `test_job_that_cannot_be_reported_is_requeued` in `daemon/test_main.py`.

### `[@ANCHOR: backup_management:COMM_json2_call]` `_json2_call(model, method_name, svc_uid=None, **kwargs)`

**Purpose.** One JSON-2 call to Odoo (`POST {ODOO_URL}/json/2/<model>/<method>`, database `ODOO_DB`, bearer `ODOO_PASS`); every report of a job's progress and result goes
through it.

**Call interface.**

| Name | Type | Meaning |
|---|---|---|
| `model` | `str` | Odoo model, for example `backup.job`. |
| `method_name` | `str` | Public method of that model. |
| `svc_uid` | `int` or `None` | Sent as `X-Odoo-Service-Uid` when set. |
| `**kwargs` | JSON values | The method's named arguments, sent as the JSON body. |
| returns | decoded JSON | The method's result. |

**Redirects are refused** (`_OPENER` is built with `_NoRedirect`, whose `redirect_request` `[@ANCHOR: backup_management:no_redirect]` takes the request, the response, the status, the headers and the target address and returns `None` for every redirect, so urllib raises the 3xx as an `HTTPError`): urllib would copy the bearer key to whatever host a 3xx answer names, so a 3xx surfaces as an HTTP error (`OdooAPIError`, code 3xx). Tested by `TestJson2CallRefusesARedirect` with two real local servers.

Errors: `OdooAPIError` for an HTTP error (the message starts `JSON-2 API HTTP Error <code>:`), a connection or parse failure, or an `error` member in the answer.

**Special concerns.** Blocking, 15 second timeout, no retry except one: after an HTTP 401 it calls `_reload_odoo_key()` and repeats the call once with the new key when the
key changed. The retry is safe for the calls this daemon makes because a 401 means Odoo refused before running anything. Secrets: the key is never logged.

**See also.** `test_json2_call_reloads_a_rotated_key` in `daemon/test_main.py`.


### `[@ANCHOR: backup_management:describe_command]` `_describe_command(cmd)`

**Purpose.** The backup worker logs what it is about to run. A command line can carry a secret and a log file outlives the process, so the log line gets this
summary instead of the command: the program's base name and the number of arguments. Called by `execute_job` (the "Executing" line) and by
`_run_pgbackrest_via_sidecar` (the "Delegating" line) in `daemon/main.py`.

| Argument | Type | Meaning |
| --- | --- | --- |
| `cmd` | non-empty `list[str]` | The argv about to be run; borrowed, not changed. `cmd[0]` is the program. |
| returns | `str` | `"<basename of cmd[0]> with <N> argument(s)"`, where N is `len(cmd) - 1`. |

Errors: `IndexError` for an empty list (callers have already checked `cmd[0]` with `shutil.which`). **Special concerns.** Pure: no I/O, no locks, safe to retry. Never
include an argument in the result.

**See also.** `test_describe_command_gives_the_program_and_an_argument_count_only` in `daemon/test_main.py`.

### `[@ANCHOR: backup_management:kopia_config_file]` `_kopia_config_file(config_id)`

**Purpose.** The backup worker serves many `backup.config` records, each possibly pointing at a different bucket with different credentials. Kopia keeps one global default config, which would make every job run against whichever bucket the previous job connected to, so each config gets its own kopia config file. This returns that file's path; for an S3 or B2 config `execute_job` passes it to kopia as `KOPIA_CONFIG_PATH` and as the config file of the repository connect-or-create step (`daemon/main.py`).

| Argument | Type | Meaning |
| --- | --- | --- |
| `config_id` | `int` | The id of the `backup.config` the job belongs to. |
| returns | `str` | `<_KOPIA_CONFIG_DIR>/config_<config_id>.config`. `_KOPIA_CONFIG_DIR` is `BACKUP_WORKER_KOPIA_CONFIG_DIR` from the environment, default `/var/lib/odoo/backups/.kopia-configs`. |

Errors: none raised. **Special concerns.** Pure: it builds a string, touches no file and takes no lock, so it is safe to call repeatedly; the directory and the file are created later by kopia. The id is an integer from the job payload, never a user-supplied path fragment.

**See also.** `test_01_kopia_s3_job_connects_before_snapshot_and_hides_secrets` in `daemon/test_main.py`.

### `[@ANCHOR: backup_management:worker_run]` `run()`

**Purpose.** The entry point of the backup worker service (`backup.worker.service`). It runs `main()` inside `reporting_failures("backup.worker.service")` so that an uncaught exception is sent to the exception tickets and then raised again; the process still dies and systemd (`Restart=always`, then `OnFailure=`) still sees the failure. Called from the `__main__` block of `daemon/main.py`, after the `--start-test` smoke test has been handled.

No arguments and no return value: it returns only when `main()` returns, which it does not under normal operation (it consumes the queue forever). Errors: whatever `main()` raises is raised again unchanged after being reported; `SystemExit` from a stop signal is not reported. **Special concerns.** Blocking for the life of the process. When the reporting helper `exception_reporter.py` has not been deployed to the host, the import falls back to a plain pass-through and the daemon still runs and still raises. The report carries the unit name and a journal tail with addresses removed; no argument or secret is added by this function.

**See also.** `test_an_uncaught_exception_is_reported_with_the_unit_name_and_raised_again`, `test_a_stop_signal_is_not_reported` and `test_without_the_helper_the_daemon_still_runs_and_still_raises` in `daemon/test_main.py`.

### `[@ANCHOR: backup_management:pgbackrest_sidecar_main]` `main()` (`daemon/pgbackrest_sidecar.py`)

**Purpose.** The privileged pgBackRest sidecar is started by `hams-pgbackrest-backup.path` whenever a request file appears in the spool directory. `main()` is its whole run: it makes sure the spool directory exists, lists every `request-*.json` file in it in sorted order, and hands each to `_process_one`. It handles all pending files, not only the one that triggered the unit, because several may have queued while an earlier run was busy, and then exits (the unit is `Type=oneshot`).

No arguments, no return value. The spool directory is `PGBACKREST_SPOOL_DIR` (default `/opt/hams/backup_requests`). Errors: a failure inside one request is handled by `_process_one` (it writes a `failed-<job>.json` file) and does not stop the others; `os.makedirs` can raise `OSError` when the directory cannot be created, which ends the run with a traceback. **Special concerns.** Runs as root, unsandboxed by design; each request is validated by `_process_one` before the privileged `runuser` call, and pgBackRest itself never runs as root. Not re-entrant: a second instance would race on the same request files, which the `.path` unit avoids by not starting a second oneshot while one runs.

**See also.** `test_drains_every_queued_request` in `daemon/test_pgbackrest_sidecar.py`.

### `[@ANCHOR: backup_management:get_fernet]` `BackupConfig._get_fernet()`

**Purpose.** Gives the encryption helper the key that protects the Kopia password and the storage secret key stored on a `backup.config`. Used by `_crypt_field` and so by every compute and inverse of the encrypted fields.

| Item | Meaning |
| --- | --- |
| arguments | none beyond `self`; it does not read the record. |
| returns | A `cryptography.fernet.Fernet` built from the environment variable `ODOO_BACKUP_CRYPTO_KEY`, or when that is empty `HAMS_CRYPTO_KEY`; `None` when neither is set. |
| raises | `ValueError` when the key is set but is not a valid Fernet key (32 url-safe base64 bytes). |

**Special concerns.** Reads the process environment on every call (no caching), so a changed key takes effect at once. The key is never logged or stored. A `None` result means no key is configured; `_crypt_field` then refuses to store the secret instead of saving it in the clear. Pure apart from the environment read; safe to retry.

**See also.** `test_kopia_password_round_trips_through_real_encryption` and `test_no_crypto_key_configured_raises_rather_than_silently_discarding` in `tests/test_backup_crypto.py`.

### `[@ANCHOR: backup_management:get_executable]` `BackupConfig._get_executable(engine)`

**Purpose.** Finds the backup program for an engine, and for Kopia installs it when it is missing. No production code path calls it today: the only caller is the test, and the worker daemon does its own `shutil.which` check in `execute_job` without asking Odoo. It stays as the documented way to resolve a backup binary from Odoo code.

| Argument | Type | Meaning |
| --- | --- | --- |
| `engine` | `str` | `"kopia"` or `"pgbackrest"`. |
| returns | `str` | The full path of the binary: `shutil.which(engine)` when it is on `PATH`, otherwise, for Kopia only, the path returned by `binary.manifest.ensure_executable("kopia")`. |

Errors: `UserError` when `pgbackrest` is missing (it needs OS packages and is never downloaded; the message tells the administrator to use the package manager) and `UserError("Unknown engine")` for any other engine that is not on `PATH`. **Special concerns.** Blocking and with side effects when Kopia is missing: it posts two chatter messages as the backup service account (`user_backup_service_internal`, never the caller) and downloads the binary through `binary_downloader` under that account with `active_test=False`. Retrying after a success finds the binary on `PATH` and does nothing further. The worker daemon performs its own `shutil.which` check and does not call this method.

**See also.** `test_08d_kopia_auto_download` in `tests/test_backup.py`.

### `[@ANCHOR: backup_management:publish_to_rabbitmq]` `publish_to_rabbitmq(env, msg, job_id=None, svc_uid=None)`

**Purpose.** Sends one backup or restore task to the `backup_tasks` queue through the shared `hams_rabbitmq.pool`. Called by `backup.config` (when a backup is triggered) and by the restore wizard. The pool reports success as soon as the send is queued for after the commit, so on its own it cannot say whether the broker took the message; with a job id this function turns a failed send into a failed job instead of a job that stays "pending" forever.

| Argument | Type | Meaning |
| --- | --- | --- |
| `env` | `odoo.api.Environment` | Used for the pool and, for the failure callback, the registry. Borrowed. |
| `msg` | `str` | The JSON task body, passed through unchanged. |
| `job_id` | `int` or `None` | The `backup.job` this message starts. With `svc_uid` it enables the failure callback. |
| `svc_uid` | `int` or `None` | A service user allowed to write that job; the callback runs as this user. |
| returns | `None` | Nothing. |

Errors: none escape. A publish exception is logged with its traceback; when a callback exists it is then called with `False`, and an error inside that callback is logged too. **Special concerns.** The real send happens after the transaction commits; the callback opens its own cursor from `env.registry` (the originating transaction is over by then) and calls `backup.job._mark_dispatch_failed()`. It does not retry; a failed send leaves a failed job for the administrator to see and trigger again.

**See also.** `test_01_no_channel_marks_the_job_failed` in `tests/test_backup_publish_failure.py`.
