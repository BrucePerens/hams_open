# Story: Failure Reporting [@ANCHOR: backup_management:COMM_story_failure_reporting]

This story describes how the system alerts operators when backup operations fail or snapshots become stale.

## Background
Reliable backups are critical. If a backup fails or hasn't run recently, the SRE team must be notified immediately.

## The Process
1. **Detection**:
   - **CLI Failure**: If a subprocess call to an engine fails `[@ANCHOR: backup_management:COMM_backup_trigger_execution]`.
   - **Staleness**: If no new snapshots are detected for more than 26 hours.
   - **Size Anomaly**: If a snapshot is suspiciously small (under `minimum_size_mb`).
2. **Alerting**:
   - The module uses a hard-dependency on `pager_duty` via the manifest.
   - It invokes `pager.incident.report_incident()` `[@ANCHOR: backup_management:COMM_backup_pager_synergy]`.
3. **Escalation**: The incident is reported using the `pager_service_internal` micro-account, triggering the configured escalation policy in PagerDuty.

## Verification
This behavior is verified by simulating failures in tests `[@ANCHOR: backup_management:COMM_test_backup_cron]`.

### Additional Internal Anchors
We also have some other features.
- `[@ANCHOR: backup_management:COMM_action_apply_policies_overwrite]` 
Next:
- `[@ANCHOR: backup_management:COMM_backup_worker_stdout_reading]`
Next:
- `[@ANCHOR: backup_management:COMM_backup_dashboard_tour]`
Next:
- `[@ANCHOR: backup_management:COMM_audit_ignore_catch_all_1]`
Next:
- `[@ANCHOR: backup_management:COMM_audit_ignore_catch_all_2]`
Next:
- `[@ANCHOR: backup_management:COMM_audit_ignore_sleep_1]`
Next:
- `[@ANCHOR: backup_management:COMM_audit_ignore_sleep_2]`
Next:
- `[@ANCHOR: backup_management:COMM_audit_ignore_catch_all_3]`
Next:
- `[@ANCHOR: backup_management:COMM_audit_ignore_sleep_3]`
Next:
- `[@ANCHOR: backup_management:COMM_audit_ignore_sleep_4]`
