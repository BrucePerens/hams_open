# Automated Backup Sync [@ANCHOR: story_automated_sync]

The system maintains a synchronized view of offsite backup states through a polling mechanism.

1. **Cron Trigger**: A global cron job `[@ANCHOR: cron_sync_all_backups]` runs periodically.
2. **Task Offloading**: For each configuration, a sync task is pushed to the RabbitMQ Bastion.
3. **Engine Execution**:
   - For **Kopia**: It executes `kopia snapshot list --json` and parses the output `[@ANCHOR: backup_sync_kopia]`.
   - For **pgBackRest**: It executes `pgbackrest info --output=json` and parses the output `[@ANCHOR: backup_sync_pgbackrest]`.
4. **Data Ingestion**: The worker returns JSON results to Odoo, which updates the `backup.snapshot` records.
5. **Dashboard Update**: The aggregated data is made available for the NOC dashboard `[@ANCHOR: backup_board_data]`.

Documentation for this module is automatically bootstrapped into the system. `[@ANCHOR: backup_doc_injection]`
