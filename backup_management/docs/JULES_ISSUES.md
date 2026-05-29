# Jules Provisioning and Testing Issues

## Provisioning Issues

- `IN_JULES_VM=1 python3 tools/test.py --provision-jules` timed out after 400 seconds.
  - However, subsequent checks showed that `postgresql-client` and `odoo` were eventually installed.
- Subsequent run with `--already-provisioned` initially failed with `❌ ERROR: Could not find PostgreSQL binary: initdb` because it was not in the PATH during sudo execution.
  - Manual verification confirmed `initdb` is at `/usr/lib/postgresql/18/bin/initdb`.

## Test Issues

- Standard tests for `backup_management` passed (0 failed, 0 error(s) of 26 tests).
- Some non-fatal errors and warnings were observed in the logs:
  - `WARNING zero_sudo odoo.addons.pager_duty: An error occurred: [Errno 13] Permission denied: '/app/pager_duty/daemon/pager_config.json'`
  - `ERROR/3 Unexpected indentation` in some XML/RST parsing.
