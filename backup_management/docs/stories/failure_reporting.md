# Story: Failure Reporting [@ANCHOR: backup_management:COMM_story_failure_reporting]

This story describes how the system alerts operators when backup operations fail or snapshots become stale.

## Background
Reliable backups are critical. If a backup fails or hasn't run recently, the SRE team must be notified immediately.

## The Process
1. **Detection**:
   - **CLI Failure**: If a subprocess call to an engine fails (see `COMM_backup_trigger_execution` below).
   - **Staleness**: If no new snapshots are detected for more than 26 hours.
   - **Size Anomaly**: If a snapshot is suspiciously small (under `minimum_size_mb`).
2. **Alerting**:
   - The module uses a hard-dependency on `pager_duty` via the manifest.
   - It invokes `pager.incident.report_incident()` (see `COMM_backup_pager_synergy` below).
   - "PagerDuty" and `pager_duty` here mean this project's own `pager_duty` Odoo module, not the commercial service. In the backup module the entry point is `report_backup_failure()`, which opens a critical `pager.incident` and posts the message on the configuration's chatter; the staleness and size rules are stated precisely under Health Monitoring in the [module README](../../README.md) (for example, a configuration with no recorded snapshot at all is not alerted on, and the size check applies only when **Minimum Size (MB)** is above 0).
3. **Escalation**: The incident is reported using the `pager_service_internal` micro-account, triggering the configured escalation policy in PagerDuty.

## Verification
This behavior is verified by simulating failures in the cron tests (see `COMM_test_backup_cron` below).

## Entries

Each entry below is the reference for one anchor named in this story. Where the code carrying the tag is a
call, a comment or a test, the entry says so.

### `action_trigger_backup(self)` [@ANCHOR: backup_management:COMM_backup_trigger_execution]

Purpose: the "Run Backup Now" action of `backup.config` (`backup_management/models/backup_config.py`), called from
the form button, from `cron_trigger_scheduled_backups` and over JSON-2 (it is `@public_rpc`). It does not run the
engine; per ADR 0071 (asynchronous bastion pattern) it queues a job for the backup worker daemon, which runs
`kopia snapshot create` or `pgbackrest backup`. The CLI failure of the story is detected in that daemon, not here.

| Argument | Type | Meaning |
|---|---|---|
| `self` | `backup.config` recordset | Zero or more configurations; each gets its own job. |

Returns `True`, or, when exactly one configuration is passed and `_publish_to_worker` returned a window action, that
action dict (opens the new `backup.job` form). Errors: `AccessError` from `_publish_to_worker` for a user who is not a
Backup Administrator or the service account; `UserError` from `validate_backup_path` for an unsafe local target path.
Side effects: one `backup.job` row per configuration in state `pending` and a RabbitMQ publish queued for after the
commit. Not idempotent: calling twice queues two backups. Tests: `tests/test_backup.py` line 185 (cites this tag) and
`test_11_trigger_kopia_and_pgbackrest`.

### `report_backup_failure(self, message)` [@ANCHOR: backup_management:COMM_backup_pager_synergy]

Purpose: the single alert path for every backup problem: a failed job reported by the worker, a stale configuration,
a too-small snapshot, a run of failed jobs. It is on `backup.config` and is `@public_rpc` so the daemon can call it
with its service key. "Pager" is this project's own `pager_duty` module.

| Argument | Type | Meaning |
|---|---|---|
| `message` | `str` | Human-readable description. Becomes the incident description and the chatter body. The daemon sends the last 500 characters of the engine log for CLI failures. |

Behaviour, in order: (1) as the `pager_duty.user_pager_service_internal` account, calls
`pager.incident.report_incident({"source": "Backup Manager: <name>", "severity": "critical", "description": message})`;
any exception there is logged as a warning and swallowed, because alerting must never block the audit trail (the
`audit-ignore-catch-all` comment explains that pager_duty's exception surface cannot be enumerated here); (2) as
`backup_management.user_backup_service_internal`, posts `message` on the configuration's chatter. It returns nothing.
Incidents are deduplicated by source, so repeated calls keep one open incident that `pager.incident` re-pages. The
message may contain engine output, so callers must not put secrets in it.
Tests: `tests/test_backup.py::test_04_cron_trigger` (cites this tag).

### Backup cron and failure tests [@ANCHOR: backup_management:COMM_test_backup_cron]

This tag is a test and verification marker, not a function. It is placed on `cron_sync_all_backups`,
`action_sync_snapshots`, `report_backup_failure`, on the cron records in `data/cron.xml` and
`views/backup_job_views.xml` (as `audit-ignore-cron` justifications) and on the raw SQL that finds each
configuration's latest snapshot start time. The test `test_04_cron_trigger` in `tests/test_backup.py` creates a
snapshot 30 hours old, runs `cron_sync_all_backups` and asserts that the configuration's chatter gained a message (the
failure report) and that a job exists. It does not assert on the pager incident itself, and the size rule has no test
in it; to-do: assert the incident and add a size-anomaly test. If you change the 26-hour rule, the size
rule or the alert text, change that test and the README "Health Monitoring" section in the same pull request.

### Apply policies overwrite [@ANCHOR: backup_management:COMM_action_apply_policies_overwrite]

Names the behaviour of `backup.config.action_apply_policies` (the code carries `COMM_backup_apply_policies`; this tag
exists only on the test). The method queues one `kopia_policy` job per Kopia configuration, which the worker turns into
`kopia policy set -- <target> --keep-latest/--keep-daily/--keep-weekly/--keep-monthly=<n> [--add-ignore=<pattern>]`.
It overwrites the repository policy with the configuration's `keep_*` and `exclude_patterns` values, it does not merge
them. pgBackRest configurations are skipped, because policies are a Kopia concept. Returns `True` or, for a single
record, the job action dict. The test `tests/test_tdd_fixes.py::test_action_apply_policies_overwrite` calls
`action_trigger_backup` and `action_apply_policies` on two configurations and on one. Unknown: no code outside the test
carries this tag; to-do: move the tag onto the method and remove the duplicate.

### Worker stdout reading [@ANCHOR: backup_management:COMM_backup_worker_stdout_reading]

Describes how `daemon/main.py::execute_job` collects engine output. For the direct engines it starts the command with
`subprocess.Popen(cmd, stdout=PIPE, stderr=STDOUT, text=True, shell=False)` and reads `proc.stdout.read(4096)` in a
loop until end of file. Each chunk goes to an in-memory `log_buffer` and an `unsent_buffer`; at most every 2 seconds the
unsent part is sent with `backup.job.append_log` and cleared (a `URLError` is logged and the text stays buffered). After
the loop the exit code decides the job state (`done` for 0, else `failed`) and the remainder is sent once. The sidecar
path for pgBackRest skips this loop and sends the whole captured output once. stderr is merged into stdout, so
secrets passed in the environment must never be echoed by an engine. The loop blocks the consuming thread. The tag is
only on the test `test_backup_worker_stdout_reading` (`tests/test_tdd_fixes.py`), which is a placeholder
(`assertIsNotNone(self.env)`); to-do: put the tag at the loop and write a real test with a fake engine.

### Backup dashboard tour [@ANCHOR: backup_management:COMM_backup_dashboard_tour]

The browser tour `backup_dashboard_tour` in `static/src/tests/tours/backup_dashboard.js`, run by
`tests/test_tour.py::test_backup_dashboard_tour` as admin through `start_tour("/odoo?debug=1", ...)`. It opens the
Backup Management app, creates a Kopia configuration named "Test Kopia Tour" with target path
`/var/lib/odoo/backups/tour_repo`, saves it, clicks "Backup Now" (`action_trigger_backup`), opens the Backup Dashboard and
clicks Refresh Board. A following Python check calls `action_sync_snapshots` on a pgBackRest configuration and asserts a
`pending` `backup.job` exists. The tour is a test only; it changes only test data. (The JS comment spells the tag
without the module prefix, `COMM_backup_dashboard_tour`; to-do: use the full form there.)

### Catch-all handler, per job [@ANCHOR: backup_management:COMM_audit_ignore_catch_all_1]

Marks the primary exception wrapper of the worker: `except Exception` at the end of `execute_job` in
`daemon/main.py`, annotated `audit-ignore-catch-all`. `MemoryError` is re-raised first so the supervisor restarts a
broken process. Any other exception fails only that job: it is logged with traceback, the job is written
`state=failed`, the text `Worker Error: <e>` is appended to its log and `report_backup_failure` is called with
`Worker Error (<Type>): <e>`. The broker message is then acknowledged so the worker keeps consuming, except when the
failure was `OdooUnavailableError` (see the secondary wrapper), where it is requeued. The tag itself appears only in
the test file; to-do: move it to the handler.

### Catch-all handler, failure reporting [@ANCHOR: backup_management:COMM_audit_ignore_catch_all_2]

Marks the secondary wrapper: the `except Exception as inner_e` around the reporting calls inside the primary handler
of `execute_job`. Reporting must never kill the daemon (a body that is valid JSON but not an object raises
`AttributeError` there, for example). It logs the traceback and sets `report_failed` when either error was an
`OdooUnavailableError`. In that case Odoo cannot record the job or the failure, so the message is `requeue()`d, after
a `REQUEUE_DELAY_SECONDS` (20 s) back-off, instead of being acknowledged and lost; this was added after jobs were lost
in a key rotation (ticket 116). Otherwise the message is acknowledged. Tag appears only in tests; to-do: move it.

### Catch-all handler, RPC and publish [@ANCHOR: backup_management:COMM_audit_ignore_catch_all_3]

Marks the tertiary wrapper, the catch-all on the Odoo/RPC side. Unknown which line the original tag meant: the
current candidates are `publish_to_rabbitmq` in `models/utils.py` (`except Exception` around the pool publish, which
logs, then calls `on_result(False)` so `_mark_dispatch_failed` fails the job) and `report_backup_failure`'s pager
`except Exception` in `models/backup_config.py`. Both swallow an error deliberately so that alerting or publishing
cannot raise into a user transaction. To-do: decide which one this tag belongs to and place the tag there; the tag
exists only in `tests/test_tdd_fixes.py` today.

### Main-loop sleep: RabbitMQ offline [@ANCHOR: backup_management:COMM_audit_ignore_sleep_1]

The 5 second `time.sleep(5)` in `main()` in `daemon/main.py` after `pika.exceptions.AMQPConnectionError`. The worker
loop connects, declares the durable `backup_tasks` queue with `prefetch_count=1` and consumes; when the broker is down
it waits 5 seconds and reconnects, so the unit does not spin. The sleep is `audit-ignore-sleep` because it is the
rate limit itself. Blocks only the worker's own main thread; no locks held. Test: `test_tdd_fixes.py` placeholder.

### Main-loop sleep: Odoo API error [@ANCHOR: backup_management:COMM_audit_ignore_sleep_2]

The 10 second `time.sleep(10)` after `OdooAPIError` escapes `main()` (for example Odoo refusing the worker's key while
setting up). The log says "Retrying in 10s". Longer than the broker case, since Odoo errors tend to last longer and
each retry costs an authenticated request. Same thread, no locks. Tested only by the placeholder test.

### Main-loop sleep: AMQP protocol error [@ANCHOR: backup_management:COMM_audit_ignore_sleep_3]

The 5 second `time.sleep(5)` after any other `pika.exceptions.AMQPError` (channel closed, protocol violation, such as a
consumer timeout). Logs "RabbitMQ protocol error ... Restarting..." and goes round the loop to reconnect. Numbered
3 although it sits second in the code; the numbers follow the story's earlier wording (primary, secondary, tertiary,
quaternary), not source order. Tested by the placeholder test only.

### Main-loop sleep: unexpected local error [@ANCHOR: backup_management:COMM_audit_ignore_sleep_4]

The 5 second `time.sleep(5)` after `ValueError`, `TypeError` or `OSError` reaching `main()`. Logged with traceback
("Unexpected error in main loop ... Restarting..."), then the loop rebuilds the connection. Other exception types are not
caught here and end the process, which `run()` reports to the exception tickets and which systemd restarts
(`Restart=always`). Tested by the placeholder test only.

### Dispatch failure to the worker queue [@ANCHOR: backup_management:COMM_job_dispatch_failed]

`backup.job._mark_dispatch_failed(self)` in `models/backup_job.py`. Called after the originating transaction has
committed, when the RabbitMQ send that hands a job to the worker failed (broker down, no channel, rejection).
`publish_to_rabbitmq` passes `on_result`; on failure it opens its own cursor (`registry.cursor()`) as the service user
and calls this method, because the original transaction is over.

For every job in `self` still in state `pending` it logs an error and writes `state=failed` and appends
"[SYSTEM] Could not hand this job to the backup worker queue (RabbitMQ unavailable). It was NOT run. Trigger it again
once the queue is back." to `output_log`. A job in any other state is skipped: a worker that already took it owns the
state. Returns nothing. Without it the job would stay "Queued in RabbitMQ..." until the 6 hour pending timeout
(`_fail_stale_pending_jobs`). Test: `tests/test_backup_publish_failure.py`.
