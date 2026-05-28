# Jules Issues - backup_management

This document records issues encountered during environment provisioning and test execution for the `backup_management` module in the Jules VM environment.

## Provisioning Issues
- Initial provisioning with `--provision-jules` timed out due to the large number of APT packages being installed (Odoo and its dependencies).
- APT locks were occasionally held by background processes, requiring retries.
- Odoo package installation was initially in a half-configured state (`iF` in dpkg), but was eventually resolved.

## Test Issues
- `TestBackupSecurity.test_symlink_traversal`: Failed with `PermissionError: [Errno 13] Permission denied: '/var/lib/odoo/backups'`. The test attempts to create a directory in a system path that the `jules` user (or the `odoo` user under which tests might be running) does not have write access to in this environment.
- `TestBackupTour.test_backup_dashboard_tour`: Skipped with `Failed to detect chrome devtools port after 10.0s`. Chrome headless failed to start, possibly due to DBUS connection issues (`Failed to connect to the bus: Address does not contain a colon`).
