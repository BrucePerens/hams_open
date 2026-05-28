# Jules Issues - pager_duty

This document tracks issues encountered while provisioning and testing the `pager_duty` module in the Jules VM environment.

## Provisioning Issues
- Failed to provision system packages due to network issues and empty Odoo GPG key.
- `tools/test.py --provision-jules` failed with `subprocess.CalledProcessError`.
- `gpg: no valid OpenPGP data found` when attempting to dearmor `/tmp/odoo.key`.
- `Network partition fallback safety hit fetching https://nightly.odoo.com/odoo.key: <urlopen error [Errno -3] Temporary failure in name resolution>`.
- `initdb` and other PostgreSQL binaries (except client) are missing because `postgresql-18` (server) was not installed due to provisioning failure.

## Testing Issues
- Tests could not be run because the environment was not successfully provisioned.
