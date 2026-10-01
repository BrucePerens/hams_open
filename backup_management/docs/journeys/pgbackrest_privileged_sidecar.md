# Journey: pgbackrest Backup via the Privileged Sidecar [@ANCHOR: backup_management:COMM_pgbackrest_privileged_sidecar]

`backup.worker.service` runs as `odoo` under `NoNewPrivileges=true`/`ProtectSystem=strict` and cannot read
PostgreSQL's own `0700 postgres:postgres` data directory, so it cannot run a real `pgbackrest backup` itself.
See ADR 0103 (`hams_shared/docs/adrs/0103_pgbackrest_privileged_backup_sidecar.md`) for the full architecture
decision; this journey walks through what actually happens end to end.

1. **Trigger**: an admin clicks "Backup Now" on a pgbackrest-engine `backup.config`, or its scheduled cron
   fires. Either way, `backup_worker`'s `execute_job()` builds the real pgbackrest argv (stanza, type,
   retention, and -- for S3/B2 storage -- the non-secret `--repo1-s3-*` flags).
2. **The privilege gate**: `[@ANCHOR: backup_management:COMM_pgbackrest_requires_sidecar]` recognizes this
   is a `pgbackrest backup` (not `info`, which stays on the direct-subprocess path since it already works
   fine as `odoo`) and routes it to the sidecar instead of `subprocess.Popen`.
3. **Handoff**: `_run_pgbackrest_via_sidecar()` validates the stanza name, then writes a request file
   (the argv plus, only for S3/B2, the two secret env var values) into `/opt/hams/backup_requests` --
   `backup_worker`'s own `0700 odoo:odoo` spool directory -- and polls for a result.
4. **The sidecar wakes**: `hams-pgbackrest-backup.path` notices the new `request-*.json` file and starts
   `hams-pgbackrest-backup.service`, a deliberately unsandboxed, root-started oneshot unit running
   `backup_management/daemon/pgbackrest_sidecar.py`.
5. **Re-validation**: `[@ANCHOR: backup_management:COMM_pgbackrest_sidecar_validate_request]` and
   `[@ANCHOR: backup_management:COMM_pgbackrest_sidecar_validate_env]` re-check the request from scratch --
   an exhaustive argv-prefix allowlist, a stanza regex, and an explicit allowlist of the only two env keys
   the sidecar will ever pass through -- before trusting anything `backup_worker` sent.
6. **The real backup runs**: `[@ANCHOR: backup_management:COMM_pgbackrest_sidecar_process_one]` runs
   `runuser -u postgres -- pgbackrest backup ...`, writes the result (exit code and output) back into the
   spool directory as a world-readable file, and deletes the request (shortening any S3/B2 secret's lifetime
   on disk).
7. **Completion**: `backup_worker`'s poll loop picks up the result file, reports the job's final state to
   Odoo the same way a direct-subprocess job would, and `backup.config.action_sync_snapshots()` runs to
   refresh the Snapshots tab -- indistinguishable from the admin's point of view from a backup that ran
   without this privilege boundary at all.
