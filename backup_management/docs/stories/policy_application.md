# Story: Policy Application [@ANCHOR: story_policy_application]

This story describes how retention policies are applied to backup engines from within Odoo.

## Background
Operators need to manage how many snapshots are kept (daily, weekly, monthly) to balance safety and storage costs.

## The Process
1. **Configuration**: The user sets retention values (e.g., `keep_daily`, `keep_weekly`) on the Backup Configuration form.
2. **Application**: The user clicks "Apply Policies" `[@ANCHOR: backup_apply_policies]`.
3. **Execution**: Odoo translates these settings into engine-specific commands (e.g., `kopia policy set`) and executes them via subprocess.
4. **Verification**: The system confirms the command was successful and logs the output.

## Verification
The command generation and execution are verified in `[@ANCHOR: test_apply_policies]`.
