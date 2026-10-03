# Story: Restoring a Backup Snapshot [@ANCHOR: backup_management:COMM_story_restore_wizard]

As a **Backup Administrator**,
I want a guided, safe way to restore a snapshot,
so that I never have to hand-run a raw `kopia`/`pgbackrest` restore command against production.

## The Process

1. An administrator opens the Restore Wizard from a `backup.snapshot` record. The wizard shows the
   snapshot's own backup engine (Kopia or pgBackRest) read-only, purely so the view can warn loudly
   when the pgBackRest case below applies `[@ANCHOR: backup_management:restore_wizard_engine]`.
2. Clicking "Restore" is gated to `group_backup_admin` and, for a Kopia snapshot, queues a real
   restore job to the sandboxed `backup_worker.service` `[@ANCHOR: COMM_backup_trigger_restore]`.
3. A pgBackRest restore through this wizard is refused outright, loudly, every time: the restore
   path is never actually routed through the privileged sidecar that can write PostgreSQL's real
   data directory, and has no forced-safe restore destination of its own. Rather than silently
   attempt (and fail, or write somewhere unintended) a restore it cannot safely perform, the wizard
   raises immediately -- a manually-run, carefully-scoped `pgbackrest` invocation against a
   dedicated scratch directory, per the deployment's own disaster-recovery runbook, is the only
   currently-safe path for a pgBackRest restore
   `[@ANCHOR: backup_management:restore_wizard_refuses_pgbackrest]`.

## Verification
`[@ANCHOR: backup_management:COMM_test_restore_action]`.
