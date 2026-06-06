# JULES ISSUES - backup_management

## Missing Resources
- None identified.

## Framework Bugs
- None identified.

## Test Environment Hurdles
- RabbitMQ sometimes takes longer than 5 seconds to start in the Jules VM environment, causing the test runner to report errors (though it usually succeeds on the second attempt or later).
- Initial Chrome headless start-up can be flaky in the VM, but subsequent runs are stable.
- PostgreSQL service may need manual start/restart in some VM instances to ensure the socket is available.
- `/var/lib/odoo/backups` directory permissions may need to be relaxed (777) in the test environment to allow the `odoo` user to create test symlinks.
