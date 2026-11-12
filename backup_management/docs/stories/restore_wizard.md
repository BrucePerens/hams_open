# Story: Restoring a Backup Snapshot [@ANCHOR: backup_management:COMM_story_restore_wizard]

As a **Backup Administrator**,
I want a guided, safe way to restore a snapshot,
so that I never have to hand-run a raw `kopia`/`pgbackrest` restore command against production.

## The Process

1. An administrator opens the Restore Wizard from a `backup.snapshot` record. The wizard shows the
   snapshot's own backup engine (Kopia or pgBackRest) read-only, purely so the view can warn loudly
   when the pgBackRest case below applies (field `engine`, see Entries).
2. Clicking "Restore" is gated to `group_backup_admin` and, for a Kopia snapshot, queues a real
   restore job to the sandboxed `backup_worker.service` (`action_restore`, see Entries).
3. A pgBackRest restore through this wizard is refused outright, loudly, every time: the restore
   path is never actually routed through the privileged sidecar that can write PostgreSQL's real
   data directory, and has no forced-safe restore destination of its own. Rather than silently
   attempt (and fail, or write somewhere unintended) a restore it cannot safely perform, the wizard
   raises immediately -- a manually-run, carefully-scoped `pgbackrest` invocation against a
   dedicated scratch directory, per the deployment's own disaster-recovery runbook, is the only
   currently-safe path for a pgBackRest restore
   (see Entries).

## Entries

Everything is in `backup_management/models/restore_wizard.py`, the transient model `backup.restore.wizard`.

### `engine` (field) [@ANCHOR: backup_management:restore_wizard_engine]

`engine = fields.Selection(related="snapshot_id.config_id.engine", readonly=True, string="Backup Engine")`. Not stored
and not editable: it mirrors the engine (`kopia` or `pgbackrest`) of the configuration that owns the chosen snapshot.
It exists only so `views/restore_wizard_views.xml` can show a loud warning when the engine is `pgbackrest`, the case
`action_restore` refuses. Set by the ORM when `snapshot_id` is set, read by the view and by the check in
`action_restore` (which reads `snapshot_id.config_id.engine` directly, not this field). Related fields of a transient
model need no locking. The other wizard fields are `snapshot_id` (required, read-only Many2one to `backup.snapshot`,
filled from the snapshot form's "Restore Snapshot" button) and `restore_target_path` (required Char, see
`action_restore`). Test: `tests/test_batch_2.py::test_restore_wizard_validation` asserts that the field reflects the
real engine.

### `action_restore(self)` [@ANCHOR: COMM_backup_trigger_restore]

Purpose: the "Restore" button of the wizard (`@public_rpc`). It checks the caller, refuses pgBackRest, and queues a
`restore_cmd` job for the backup worker daemon instead of running anything in Odoo.

| Input | Type | Meaning |
|---|---|---|
| `self` | one `backup.restore.wizard` | `ensure_one()` is called; more than one raises. |
| `restore_target_path` | `str` field | Validated for Kopia with `validate_backup_path` (no shell metacharacters, no `..`), but then not used as the destination. |
| `snapshot_id` | `backup.snapshot` field | The snapshot to restore; its `snapshot_id` string and its configuration give the engine and storage settings. |

Order of checks: group `backup_management.group_backup_admin` or `AccessError`; pgBackRest engine or `UserError`
(see the refusal entry); `validate_backup_path` for Kopia (`UserError` on an unsafe path). Then, as
`user_backup_service_internal` and without `with_company` (the service account belongs only to the main company, so
`with_company` would raise `AccessError`), it creates a `backup.job` in state `pending` with log "Restore queued in
RabbitMQ...".

For Kopia the command is `["kopia", "restore", <snapshot_id>, "/var/lib/odoo/backups/restore_<job.id>"]`. The
destination is derived from the database-assigned job id, not from `restore_target_path` or from the snapshot label,
because both of those are free text a tenant administrator could point at another tenant's repository or at a system
path; the chosen destination is appended to the job log. `restore_target_path` stays required but is unused for Kopia;
to-do: relabel or make it optional per engine. The JSON payload carries `job_id`, `config_id`, `engine="restore_cmd"`,
`cmd_args`, `snapshot_id`, `svc_uid`, `kopia_password` and the storage fields (`storage_type`, `bucket_name`,
`endpoint_url`, `access_key`, `secret_key`) so an S3/B2 restore uses the right repository. It contains secrets, so it
is published only through `publish_to_rabbitmq` in a post-commit hook (`env.cr.postcommit`), and the daemon redacts
`access_key` and `secret_key` in its logs. If the send fails, `_mark_dispatch_failed` fails the job.

Returns an `ir.actions.act_window` dict opening the new `backup.job` form. Side effects: one job row, one queued
message. Not idempotent: each click queues a new restore into a new directory. The Kopia restore only writes into that
fresh directory, it never overwrites a live repository. Tests: `tests/test_backup_security.py::test_restore_wizard_security`
and `tests/test_backup.py::test_13_restore_action`.

### pgBackRest restore refusal [@ANCHOR: backup_management:restore_wizard_refuses_pgbackrest]

The early `if self.snapshot_id.config_id.engine == "pgbackrest": raise UserError(...)` in `action_restore`, placed
right after the group check. Every pgBackRest restore through the wizard fails with a message saying it is not yet safe,
whatever `restore_target_path` holds. Reason: the daemon's privileged sidecar only lists `backup` in
`_PGBACKREST_PRIVILEGED_OPS`, so a queued restore would run in the sandboxed `backup_worker.service` with no permission
to write PostgreSQL's data directory and with no forced-safe destination. Routing restore through a hardened sidecar is
a privilege-boundary decision that has not been made; until then the supported path is a hand-run, scoped `pgbackrest`
command against a scratch directory, per the disaster-recovery runbook. The `elif engine == "pgbackrest"` branch further
down (stanza taken from the snapshot's configuration `target_path`, never from the wizard, plus `--set=<snapshot_id>`)
is currently unreachable and is kept as the blueprint for that future work. Test:
`tests/test_batch_2.py::test_restore_wizard_validation` (a well-formed stanza is refused too).

### Restore verification [@ANCHOR: backup_management:COMM_test_restore_action]

A test marker, not a function. `tests/test_backup_security.py::test_restore_wizard_security` creates a wizard on a
Kopia snapshot with `restore_target_path` `/etc/passwd` and asserts `UserError` (an unsafe path is refused), then
creates a pgBackRest configuration and snapshot and asserts the pgBackRest refusal even for an injection-shaped
target. `tests/test_backup.py::test_13_restore_action` carries the tag next to a patched `pika.BlockingConnection`; it is the
happy path: a Kopia restore returns a `backup.job` action for the right configuration. To-do: no test yet asserts the exact Kopia `cmd_args` or the job-id based destination.
