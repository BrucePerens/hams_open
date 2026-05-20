# Backup Management

Unified Backup Management Facility for Odoo 19, orchestrating **Kopia** and **pgBackRest** with a Focus on Zero-Sudo and Multi-Website Architecture.

## Architecture Highlights
- **Hybrid Engine Support:** Native integration with Kopia (files/system) and pgBackRest (PostgreSQL WAL).
- **Asynchronous Execution:** Offloads heavy CLI operations to a RabbitMQ-backed daemon (`backup_worker`). This ensures that long-running backup or restore processes do not block the Odoo web interface.
- **Micro-Privilege Security:** Operations execute via specific service accounts; strict path validation prevents system exposure. No `sudo` is used anywhere in the module.
- **Multi-Website Isolation:** Segregates backup configurations, snapshots, and jobs by Odoo Website ID. Administrators only see data relevant to their authorized websites.
- **Self-Healing Deployment:** Automatically downloads Kopia binaries if missing, ensuring immediate operational readiness.

## Prerequisites
- **pgBackRest:** Must be installed at the OS level (e.g., `apt install pgbackrest`).
- **Kopia:** Automatically provisioned JIT via `binary_downloader` if not found.
- **RabbitMQ:** Required for the task queue.

## Configuration
1. Navigate to **Backups > Configurations**.
2. Select an engine and define the **Target Path** (or Stanza name for pgBackRest).
3. Set retention policies (Daily/Weekly/Monthly).
4. Configure storage backends (Local, S3, or B2).

## Semantic Anchors for Traceability
- `[@ANCHOR: UX_BACKUP_SYNC]`: Dashboard metadata synchronization.
- `[@ANCHOR: security_path_validation]`: Logic for validating target paths.
- `[@ANCHOR: backup_trigger_execution]`: Entry point for manual backup execution.
- `[@ANCHOR: backup_doc_injection]`: Knowledge article bootstrap.

## Operational Notes
- **Drill Scripts:** You can configure automated restore drills to verify backup integrity weekly.
- **PagerDuty Integration:** Alerts are automatically dispatched to the SRE team on backup failures or size anomalies.
