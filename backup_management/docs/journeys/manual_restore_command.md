# Manual Restore Command [@ANCHOR: backup_management:COMM_journey_manual_restore_command]

While automated restores are supported, the system provides pre-generated CLI commands for emergency manual use.

1. **Navigate**: Go to a specific Snapshot record.
2. **View Command**: The "Restore Command" field (`restore_command`, see Entries) contains the pre-formatted CLI command (e.g., `kopia restore <snapshot_id> <target>`).
3. **Execution**: The SRE can copy this command and execute it directly on the database host.
4. **Automated Restore**: Users can also use the Restore Wizard to trigger an automated restoration (`action_restore` and its test, see Entries).

## Entries

### `_compute_restore_command(self)` [@ANCHOR: backup_management:COMM_backup_restore_command]

Computes the non-stored Char field `backup.snapshot.restore_command` ("Restore Command") in
`backup_management/models/backup_snapshot.py`. It is `@api.depends("snapshot_id", "config_id.engine",
"config_id.target_path", "config_id.pgbackrest_repo")`, so the text updates when any of these change. Per record: empty
string when there is no `snapshot_id` or no configuration, or an engine it does not know. For Kopia the text is
`kopia restore <snapshot_id> /var/lib/odoo/backups/restore_<snapshot_id>`. For pgBackRest it is `pgbackrest restore
--stanza=<target_path> --set=<snapshot_id>` plus ` --repo=<n>` when the configuration names a repository. The text is
only displayed: nothing runs it, and it is not shell-quoted, so the operator must read it before pasting (the
`snapshot_id` and `target_path` come from engine output and from validated configuration). It is the pgBackRest
route the wizard refuses (see the restore wizard story), so for a database restore this command, adapted to a scratch
directory per the disaster-recovery runbook, is the supported path. Test: `test_10_restore_command_computation` in
`tests/test_backup.py`.

### `action_restore(self)` [@ANCHOR: backup_management:COMM_backup_trigger_restore]

The "Restore" button of `backup.restore.wizard` (`models/restore_wizard.py`), written in the code as
`COMM_backup_trigger_restore`. It requires `group_backup_admin`, refuses pgBackRest snapshots with a `UserError`, and
for Kopia queues a `restore_cmd` job `kopia restore <snapshot_id> /var/lib/odoo/backups/restore_<job.id>` for the
sandboxed worker, returning the job form action. The destination is derived from the job id, not from what the
administrator typed. Full entry, arguments and checks: `docs/stories/restore_wizard.md`. Tests:
`tests/test_backup.py::test_13_restore_action` and `tests/test_backup_security.py::test_restore_wizard_security`.

### Restore verification [@ANCHOR: backup_management:COMM_test_restore_action]

Test marker for the restore wizard: `tests/test_backup_security.py::test_restore_wizard_security` asserts that an
unsafe Kopia restore path and every pgBackRest restore raise `UserError`; `tests/test_backup.py::test_13_restore_action`
asserts that a valid Kopia restore returns a `backup.job` action for the right configuration (with `pika` patched, so no
broker is needed). See `docs/stories/restore_wizard.md` for what these tests do not cover.
