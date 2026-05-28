# Jules VM Testing Issues - daemon_key_manager

## Provisioning Issues
- Initial test run failed because the `odoo` user did not have permission to create directories in `/home/jules/.local`. This was resolved by setting `XDG_DATA_HOME=/tmp/odoo_data` and ensuring it has wide permissions (777).

## Test Failures
- `TestKeyRegistryTour.test_daemon_key_manager_tour` was skipped with the message: `Failed to detect chrome devtools port after 10.0s.`.
- Chrome headless failed to start with several errors:
    - `mkdir : No such file or directory (2)`
    - `Failed to connect to the bus: Address does not contain a colon`
- `TestKeyRegistry.test_documentation_installed` was skipped with the message: `No documentation model available`.
