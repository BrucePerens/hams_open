# Jules Issues - hams_test

## Provisioning Issues
- Initial provisioning of the testing environment via `./tools/test.py --provision-jules` succeeded, but took a significant amount of time (over 400 seconds) due to downloading and installing many system dependencies (Odoo, PostgreSQL, etc.).

## Test Failures
- Running standard tests for `hams_test` module (`IN_JULES_VM=1 python3 tools/test.py -u hams_test --already-provisioned`) failed with a critical error:
  ```
  odoo.exceptions.UserError: Recursion error in modules dependencies!
  ```
- Analysis of the module manifests reveals a circular dependency between `hams_test` and `zero_sudo`:
  - `hams_test` depends on `zero_sudo`
  - `zero_sudo` depends on `hams_test`
- This circular dependency prevents the Odoo registry from loading and thus stops any tests from running.
