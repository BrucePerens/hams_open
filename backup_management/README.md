# Backup Management (`backup_management`)

*Copyright © Bruce Perens K6BP. Licensed under the GNU Affero General Public License v3.0 (AGPL-3.0).*

Implements a centralized "single pane of glass" GUI in Odoo to orchestrate and monitor `kopia` and `pgBackRest` system backups. This module is designed with a hybrid architecture, using Kopia for filesystem-level backups and pgBackRest for robust PostgreSQL WAL archiving.

---

# Technical Documentation

## Architecture
* **Self-Healing Dependencies:** Uses `shutil.which` to detect tools. If `kopia` is missing from the system path, it automatically fetches and extracts the pre-compiled Linux binary into the `var/lib/odoo/ext_bin` local data directory via `binary_downloader` to ensure uninterrupted operation.
* **Kopia:** Used for file/system state. Parsed via `kopia snapshot list --json`. State is synchronized via `action_sync_snapshots` `[@ANCHOR: backup_sync_kopia]`. Retention policies are applied natively via `action_apply_policies` `[@ANCHOR: backup_apply_policies]`.
* **pgBackRest:** Used for PostgreSQL WAL archiving. Parsed via `pgbackrest info --output=json`. State is synchronized via `action_sync_snapshots` `[@ANCHOR: backup_sync_pgbackrest]`.
* **Orchestration:** Capable of pushing execution commands (`kopia snapshot create`, `pgbackrest backup`) directly to the underlying daemons via RabbitMQ offloading `[@ANCHOR: backup_trigger_execution]`. Can generate automated restore drill commands `[@ANCHOR: backup_restore_command]`.
* **Asynchronous Bastion Pattern:** Implements ADR-0071, offloading long-running CLI operations to a RabbitMQ-backed worker daemon (`backup_worker.py`) to prevent Odoo worker timeouts and ensure reliability.

## Security & Operations
* **Zero-Sudo Compliance:** Strictly adheres to Zero-Sudo architecture. All background operations and privilege elevations use the `backup_management.user_backup_service_internal` service account via `zero_sudo.security.utils`. Use of `.sudo()` is strictly prohibited. The module uses `svc_uid` to ensure operations are performed with the correct service identity.
* **Micro-Privilege Architecture:** Executes operations with minimum required privileges. The `backup_worker.py` daemon uses a dedicated service account and its keys are provisioned automatically via `daemon_key_manager`. Command execution is restricted to allowed binaries (`kopia`, `pgbackrest`) and validated paths.
* **Encryption:** Kopia passwords and other sensitive keys (S3 secret keys) are encrypted at rest using the system's `ODOO_BACKUP_CRYPTO_KEY` (or fallback `HAMS_CRYPTO_KEY`) Fernet key.
* **Path & Input Validation:** Implements strict validation for backup targets, restore paths, and pgBackRest stanzas to prevent shell injection and accidental overwriting of critical system files.
* **Pager Duty Synergy:** Employs a soft-dependency on `pager_duty`. If a CLI command fails, a snapshot size anomaly is detected, or a backup snapshot becomes stale (no new snapshots in >26 hours), the module directly invokes `pager.incident.report_incident()` `[@ANCHOR: backup_pager_synergy]` using the `pager_service_internal` micro-account to instantly alert the on-call SRE.

## Automated Subsystems & Reporting
* **Dashboard Status:** Aggregates target state and snapshot staleness for the NOC display `[@ANCHOR: backup_board_data]`.
* **Global Sync Cron:** Polling loop to synchronize offsite states `[@ANCHOR: cron_sync_all_backups]`.

## Architectural Stories & Journeys

For detailed narratives and end-to-end workflows, refer to the following:

### Stories
* [Automated Synchronization](docs/stories/automated_sync.md) `[@ANCHOR: story_automated_sync]`
* [Failure Reporting](docs/stories/failure_reporting.md) `[@ANCHOR: story_failure_reporting]`
* [Policy Application](docs/stories/policy_application.md) `[@ANCHOR: story_policy_application]`
* [Secure Path Validation](docs/stories/secure_path_validation.md) `[@ANCHOR: story_secure_path_validation]`

### Journeys
* [Backup Configuration and First Sync](docs/journeys/backup_config_sync.md) `[@ANCHOR: journey_backup_config_sync]`
* [Manual Restore Command Generation](docs/journeys/manual_restore_command.md) `[@ANCHOR: journey_manual_restore_command]`

## Testing & Verification
* **Cron Reliability:** Scheduled syncing functions are validated by `[@ANCHOR: test_backup_cron]`.
* **View Rendering:** Interface layouts and dashboards are verified by `[@ANCHOR: test_backup_view]`.
* **Subprocess Orchestration:** Shell executions are strictly mocked and verified by `[@ANCHOR: test_backup_orchestration]`.
