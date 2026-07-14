# Automated Backup Sync [@ANCHOR: backup_management:COMM_story_automated_sync]

The system maintains a synchronized view of offsite backup states through a polling mechanism.

1. **Cron Trigger**: A global cron job `[@ANCHOR: backup_management:COMM_cron_sync_all_backups]` runs periodically.
2. **Task Offloading**: For each configuration, a sync task is pushed to the RabbitMQ Bastion.
3. **Engine Execution**:
   - For **Kopia**: It executes `kopia snapshot list --json` and parses the output `[@ANCHOR: backup_management:COMM_backup_sync_kopia]`.

   - For **pgBackRest**: It executes `pgbackrest info --output=json` and parses the output `[@ANCHOR: backup_management:COMM_backup_sync_pgbackrest]`.

4. **Data Ingestion**: The worker returns JSON results to Odoo, which updates the `backup.snapshot` records. This is optimized using a Postgres stored procedure `[@ANCHOR: backup_management:COMM_upsert_snapshots_procedure]` to ensure atomic, single-roundtrip bulk updates `[@ANCHOR: backup_management:COMM_upsert_snapshots_roundtrip_optimization]`.

5. **Dashboard Update**: The aggregated data is made available for the NOC dashboard `[@ANCHOR: backup_management:COMM_backup_board_data]`.

Documentation for this module is automatically bootstrapped into the system. `[@ANCHOR: backup_management:COMM_backup_doc_injection]`

## Connection Verification [@ANCHOR: backup_management:COMM_action_test_connection]
Administrators can manually trigger a connection test and snapshot synchronization from the configuration view.

## Job Monitoring [@ANCHOR: backup_management:COMM_action_view_latest_job]
Each configuration provides a direct link to view the logs and status of its most recent backup job.

## Status Auto-Refresh [@ANCHOR: backup_management:COMM_auto_refresh_status]
The system automatically monitors active jobs and cleans up any that have been abandoned by the worker.
