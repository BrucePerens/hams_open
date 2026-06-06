# Jules Issues - Compliance Module

## Environment Hurdles
- UI Tour (`test_compliance_tour`) was initially failing due to Chrome startup issues and then due to the cookie bar blocking clicks on footer links. Fixed by adding a step to accept cookies using the `:not(:visible)` selector as per Odoo 19 headless constraints.
- `tools/test.py` does not seem to support `--provision-jules` despite `docs/TESTING_IN_JULES.md` mentioning it. Using `IN_JULES_VM=1` instead.
- PostgreSQL service in the VM requires manual startup (`pg_ctlcluster 18 main start`) before running tests.

## Missing Resources
- None identified.

## Framework Bugs
- None identified.
