# Jules VM Testing Issues - hams_helpdesk

## Provisioning Issues

- The provisioning command `IN_JULES_VM=1 python3 tools/test.py --provision-jules` timed out after 400 seconds. However, it appears that `odoo` was installed and the PostgreSQL cluster was initialized before the timeout.

## Standard Test Issues

- None. All standard tests for `hams_helpdesk` passed successfully in the Jules VM environment.
