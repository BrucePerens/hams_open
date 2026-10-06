# Story: Policy Application [@ANCHOR: backup_management:COMM_story_policy_application]

This story describes how retention policies are applied to backup engines from within Odoo.

## Background
Operators need to manage how many snapshots are kept (daily, weekly, monthly) to balance safety and storage costs.

## The Process
1. **Configuration**: The user sets retention values (e.g., `keep_daily`, `keep_weekly`) on the Backup Configuration form.
2. **Application**: The user clicks "Apply Policies" `[@ANCHOR: backup_management:COMM_backup_apply_policies]`.
3. **Execution**: Odoo translates these settings into engine-specific commands (e.g., `kopia policy set`) and executes them via subprocess.
   - This applies to Kopia only. For pgBackRest, **Apply Policies** pushes nothing: only **Keep Daily** is used, as `--repo1-retention-full` on each backup, and the weekly and monthly values are ignored (see Automated Retention in the [module README](../../README.md)).
4. **Verification**: The system confirms the command was successful and logs the output.

## Verification
The command generation and execution are verified in `[@ANCHOR: backup_management:COMM_test_apply_policies]`.
