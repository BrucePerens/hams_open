# Backup Management Module

This module provides a unified interface for managing system backups using Kopia (for files) and pgBackRest (for PostgreSQL databases). It is designed to work in multi-website and multi-company environments.

## Features

- **Multi-Engine Support:** Manage both Kopia and pgBackRest from a single dashboard.
- **Asynchronous Execution:** Backup and restore jobs are offloaded to a background worker via RabbitMQ to prevent UI blocking.
- **Auto-Refresh Status:** Backup job status is automatically updated via a periodic cron job.
- **Performance Optimized:** Bulk snapshot synchronization uses PostgreSQL stored procedures for single-roundtrip updates.
- **Multi-Tenant Aware:** Backups and snapshots are isolated by website and company.
- **Automated Retention:** Configure daily, weekly, and monthly retention policies.
- **Health Monitoring:** Automated stale backup detection and size anomaly alerts via PagerDuty.
- **Restore Drills:** Schedule automated restore tests to verify backup integrity.
- **Zero-Sudo Security:** All background operations and API calls use dedicated service accounts for maximum security.

## Technical Specification

### 1. Automated Volume Synchronization
Handles the execution loops for continuous file system snapshots and system storage mappings.
* **Core Sync Anchor:** `[@ANCHOR: backup_management:COMM_backup_sync_kopia]`

* **Database Target Sync Anchor:** `[@ANCHOR: backup_management:COMM_backup_sync_pgbackrest]`

* **Cron Routine Orchestration:** `[@ANCHOR: backup_management:COMM_cron_sync_all_backups]`

* **Performance Bulk Upsert:** `[@ANCHOR: backup_management:COMM_upsert_snapshots_procedure]`

### 2. Retention & Purge Governance
Ensures structural space recovery processes comply with multi-website tenant data privacy mandates.
* **Policy Application Engine:** `[@ANCHOR: backup_management:COMM_backup_apply_policies]`

* **Interactive Dashboard Telemetry:** `[@ANCHOR: backup_management:COMM_backup_board_data]`

## User Guide

### Configuring a Backup
1. Navigate to **Backup Management > Configuration > Backup Configurations**.
2. Click **New** to create a new configuration.
3. Select the **Engine** (Kopia or pgBackRest).
4. Enter the **Target Path** (for Kopia) or **Stanza Name** (for pgBackRest).
5. Set retention policies (Daily/Weekly/Monthly).
6. Save the configuration.

### Monitoring Backups
- The **Backup Dashboard** provides a high-level overview of the latest snapshots and their status.
- **Backup Jobs** show the history and live output logs of background operations.

### Restoring a Backup
1. Go to **Backup Management > Backups > Snapshots**.
2. Select the snapshot you wish to restore.
3. Click the **Restore** button.
4. Enter the target directory for the restore and confirm.

## Cross-Module Interfaces

### Compliance Monitoring
When multi-website context isolation checks detect data boundary leakage or cross-tenant contamination, logging structures communicate directly with the core website security system:
* **Tenant Violation Reports:** For tracking frontend moderation workflow alerts, see `[@ANCHOR: user_websites:UX_REPORT_VIOLATION]`.
* **Automated Escalation:** System telemetry monitors structural volume metrics and communicates alerts dynamically.

## External Dependencies

- `pager_duty`: Used for failure reporting and stale snapshot alerts.
