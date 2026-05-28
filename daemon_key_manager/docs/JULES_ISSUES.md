# Jules Provisioning and Testing Issues

## Provisioning Issues
- Encountered `/var/cache/debconf/tmp.ci/postgresql.config.xoma5D: 12: pg_lsclusters: not found` during Odoo installation.
- Multiple warnings about running pip as root.

## Testing Issues
- Standard tests failed to run due to a recursion error in module dependencies.
- `daemon_key_manager` depends on `zero_sudo` and `hams_test`.
- `zero_sudo` depends on `hams_test`.
- `hams_test` depends on `zero_sudo`.
- This circular dependency causes `odoo.exceptions.UserError: Recursion error in modules dependencies!` when attempting to load the modules for testing.
