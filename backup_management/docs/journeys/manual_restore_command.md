# Manual Restore Command [@ANCHOR: journey_manual_restore_command]

While automated restores are supported, the system provides pre-generated CLI commands for emergency manual use.

1. **Navigate**: Go to a specific Snapshot record.
2. **View Command**: The "Restore Command" field `[@ANCHOR: backup_restore_command]` contains the pre-formatted CLI command (e.g., `kopia restore <snapshot_id> <target>`).
3. **Execution**: The SRE can copy this command and execute it directly on the database host.
4. **Automated Restore**: Users can also use the Restore Wizard to trigger an automated restoration. `[@ANCHOR: backup_trigger_restore]`
