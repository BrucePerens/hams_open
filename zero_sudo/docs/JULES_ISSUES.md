# Jules VM Issues - zero_sudo

## Provisioning Issues
Provisioning was successful overall. The following minor non-fatal errors were observed:
- `ERROR: role "jules" already exists`: Occurred during PostgreSQL role creation, likely because the role was already present in the environment.
- `debconf: unable to initialize frontend: Dialog`: Expected in non-interactive bash sessions.
- `pg_lsclusters: not found`: Occurred during postgresql package configuration but did not prevent successful provisioning.

## Test Issues
Standard tests passed successfully (0 failed, 0 errors).

The following tests were skipped:
- `TestSecurityUtils.test_09_bootstrap_knowledge_docs`: Skipped because "No documentation API available."
- `TestZeroSudoViews.test_02_zero_sudo_tour`: Skipped because "hams_test module not installed". This is notable because the logs indicate `hams_test` WAS used as the database name and there are multiple log lines from `odoo.addons.hams_test.tests.common`, but the tour still skipped due to perceived absence of the module.
