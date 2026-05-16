# 💾 Backup Management (`backup_management`)

## Architecture
* **Self-Healing Dependencies:** Uses `shutil.which` to detect tools. If `kopia` is missing from the system path, it automatically fetches and extracts the pre-compiled Linux binary into the `var/lib/odoo/ext_bin` local data directory to ensure uninterrupted operation.
Implements a Hybrid Architecture for unified backup management.
* **Kopia:** Used for file/system state. Parsed via `kopia snapshot list --json`. State is synchronized via `_sync_kopia` `[@ANCHOR: backup_sync_kopia]`. Retention policies are applied natively via `[@ANCHOR: backup_apply_policies]`.
* **pgBackRest:** Used for PostgreSQL WAL archiving. Parsed via `pgbackrest info --output=json`. State is synchronized via `_sync_pgbackrest` `[@ANCHOR: backup_sync_pgbackrest]`.
* **Orchestration:** Capable of pushing execution commands (`kopia snapshot create`, `pgbackrest backup`) directly to the underlying daemons via `subprocess` from the UI `[@ANCHOR: backup_trigger_execution]`. Can generate automated restore drill commands `[@ANCHOR: backup_restore_command]`.

## Security & Operations
* **Service Account:** Utilizes `user_backup_service_internal` for background synchronization.
* **Encryption:** Kopia passwords are encrypted at rest using the system's `ODOO_BACKUP_CRYPTO_KEY` Fernet key via standard getter/setter properties.
* **Subprocess Execution:** Uses Python's `subprocess.run` to interrogate local CLIs.
* **Pager Duty Synergy:** Employs a soft-dependency on `pager_duty`. If a CLI command fails or a backup snapshot becomes stale (no new snapshots in >26 hours), the module directly invokes `pager.incident.report_incident()` `[@ANCHOR: backup_pager_synergy]` using the `pager_service_internal` micro-account to instantly alert the on-call SRE.
* **Size Anomaly Detection:** The config model evaluates newly ingested snapshots against `minimum_size_mb`. If an empty or suspiciously small snapshot is generated (e.g., missing Docker volume mounts), it escalates a critical alert.

## Automated Subsystems & Reporting
* **Dashboard Status:** Aggregates target state and snapshot staleness for the NOC display `[@ANCHOR: backup_board_data]`.
* **Global Sync Cron:** Polling loop to synchronize offsite states `[@ANCHOR: cron_sync_all_backups]`.

## Architectural Stories & Journeys

For detailed narratives and end-to-end workflows, refer to the following:

### Stories
* [Automated Synchronization](docs/stories/automated_sync.md)
* [Failure Reporting](docs/stories/failure_reporting.md)
* [Policy Application](docs/stories/policy_application.md)
* [Secure Path Validation](docs/stories/secure_path_validation.md)

### Journeys
* [Backup Configuration and First Sync](docs/journeys/backup_config_sync.md)
* [Manual Restore Command Generation](docs/journeys/manual_restore_command.md)

## Testing & Verification
* **Cron Reliability:** Scheduled syncing functions are validated by `[@ANCHOR: test_backup_cron]`.
* **View Rendering:** Interface layouts and dashboards are verified by `[@ANCHOR: test_backup_view]`.
* **Subprocess Orchestration:** Shell executions are strictly mocked and verified by `[@ANCHOR: test_backup_orchestration]`.
