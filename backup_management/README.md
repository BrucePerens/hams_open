import: `hams_open.backup_management`

# Backup Management Module

This module provides a unified interface for managing system backups using Kopia (for files) and pgBackRest (for PostgreSQL databases). It is designed to work in multi-website and multi-company environments.

## Features

- **Multi-Engine Support:** Manage both Kopia and pgBackRest from a single dashboard.
- **Asynchronous Execution:** Backup and restore jobs are offloaded to a background worker via RabbitMQ to prevent UI blocking. Each request first creates a `backup.job` record in state `pending`; the RabbitMQ message is sent only after the database transaction commits. If that send fails, the job is marked `failed` with a note that it was not run and must be triggered again.
- **Live Job Status:** The worker daemon writes each job's state back to Odoo as it runs (`processing`, then `done` or `failed`) and appends its output to the job log. `backup.job._auto_refresh_status()` can mark a job that has sat in `processing` for more than 2 hours as `failed`, but no cron job currently calls it.
- **Performance Optimized:** Bulk snapshot synchronization uses a PostgreSQL function (`upsert_backup_snapshots`) that inserts every snapshot from one sync in a single statement, skipping snapshots already recorded, and returns only the new ones.
- **Multi-Tenant Aware:** Backups and snapshots are isolated by website and company.
- **Scheduled Backups:** A daily cron job ("Backup Management: Scheduled Backup Run") queues a backup for every configuration. A separate hourly cron job ("Backup Management: Sync Snapshots") only refreshes the snapshot list; it never creates a backup.
- **Automated Retention:** Configure daily, weekly, and monthly retention policies. For Kopia they are pushed to the repository (`kopia policy set`) only when an administrator clicks **Apply Policies**. For pgBackRest only **Keep Daily** is used, as `--repo1-retention-full` on each backup; the weekly and monthly values are ignored.
- **Health Monitoring:** Automated stale backup detection and size anomaly alerts via PagerDuty (this project's `pager_duty` Odoo module, not the commercial service): `report_backup_failure()` opens a critical `pager.incident` and posts the message on the configuration's chatter. The hourly sync cron alerts when a configuration's newest recorded snapshot started more than 26 hours ago (a configuration with no recorded snapshot at all is not alerted on), and each newly recorded snapshot smaller than the configuration's **Minimum Size (MB)** raises an anomaly alert (only when that field is above 0).
- **Restore Drills:** Automated restore tests to verify backup integrity. If a configuration names a **Restore Drill Script**, the hourly sync cron queues it whenever the last successful drill is more than 7 days old. The worker runs the script only if it is an executable `.py` file under `BACKUP_WORKER_SCRIPTS_DIR` (default `/opt/hams/daemons/backup_worker/scripts`), and records **Last Successful Drill** when it exits with code 0.
- **Zero-Sudo Security:** All background operations and API calls use dedicated service accounts for maximum security. "Zero-Sudo" is this codebase's rule (enforced by the `zero_sudo` module) that Odoo's `.sudo()` is never used; privileged steps instead run as a named service account, here `backup_service_internal`, resolved with `zero_sudo.security.utils._get_service_uid()`.

## Technical Specification

### 1. Automated Volume Synchronization
Copies the snapshot list reported by Kopia (`kopia snapshot list`) or pgBackRest (`pgbackrest info`) into `backup.snapshot`, from the hourly cron job or the **Sync Snapshots** button.
* **Core Sync Anchor:** `[@ANCHOR: backup_management:COMM_backup_sync_kopia]`

* **Database Target Sync Anchor:** `[@ANCHOR: backup_management:COMM_backup_sync_pgbackrest]`

* **Cron Routine Orchestration:** `[@ANCHOR: backup_management:COMM_cron_sync_all_backups]`

* **Performance Bulk Upsert:** `[@ANCHOR: backup_management:COMM_upsert_snapshots_procedure]`

### 2. Retention & Purge Governance
Applies Kopia retention and exclusion policies (**Apply Policies**), and supplies the per-configuration data shown on the Backup Dashboard.
* **Policy Application Engine:** `[@ANCHOR: backup_management:COMM_backup_apply_policies]`

* **Interactive Dashboard Telemetry:** `[@ANCHOR: backup_management:COMM_backup_board_data]`

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
- **pgBackRest Sidecar:** `pgbackrest backup` is the one command the daemon does not run itself, because it needs filesystem access to PostgreSQL's data directory that the daemon's own account does not have. The daemon writes a request file into `PGBACKREST_SPOOL_DIR`; a separate privileged sidecar (`daemon/pgbackrest_sidecar.py`) validates it, runs pgBackRest as the `postgres` user, and writes a result file back. The daemon waits up to `PGBACKREST_SIDECAR_TIMEOUT` seconds (default 3600) and fails the job if no result arrives.
- **Communication:** It connects back to Odoo via the JSON-2 API (e.g., `/json/2/backup.job/write`) using a dedicated internal service account (`backup_service_internal`, authenticated with the API key in the `ODOO_RPC_KEY` environment variable) to stream log buffers, update statuses, and report failures.
- **Failure Handling:** An error while processing one message fails only that job: the daemon marks it `failed`, appends the error to its log, calls `report_backup_failure()`, and acknowledges the message so it is not redelivered.
- **Security:** The daemon rigidly restricts allowed commands and targets. It enforces bounds-checking, blocks malicious parameters, and ensures path isolation before making subprocess calls.

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
* **Tenant Violation Reports:** For tracking frontend moderation workflow alerts, see `[@ANCHOR: user_websites:UX_REPORT_VIOLATION]`.
* **Automated Escalation:** Backup failures, stale backups and undersized snapshots are escalated through `report_backup_failure()` to the `pager_duty` module (see Health Monitoring above).

## External Dependencies

- `pager_duty`: Used for failure reporting and stale snapshot alerts.
- `pika`: Used for RabbitMQ integration in the daemon.
- `cryptography`: Used for Fernet encryption of the stored Kopia password and S3/B2 secret key.
