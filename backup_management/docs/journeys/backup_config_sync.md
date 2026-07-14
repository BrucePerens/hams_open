# Journey: Backup Configuration and First Sync [@ANCHOR: backup_management:COMM_journey_backup_config_sync]

This journey walks through the initial setup of a backup target and its first metadata synchronization.

1. **Create Configuration**: The administrator navigates to **Ham Admin > Backups > Configurations** and creates a new record ([@ANCHOR: backup_management:COMM_backup_dashboard_tour]).
2. **Set Parameters**: They choose the engine (Kopia), set the target path (e.g., `/var/lib/odoo/backups/repo`), and provide the encryption password.
3. **Save**: Upon saving, the system validates the paths and encrypts the password.
4. **Trigger Sync**: The admin clicks "Sync Snapshots".
5. **Execution**: Odoo executes the engine-specific list command `[@ANCHOR: backup_management:COMM_backup_sync_kopia]`.
6. **Result**: The "Snapshots" tab populates with the list of available backups retrieved from the engine.
