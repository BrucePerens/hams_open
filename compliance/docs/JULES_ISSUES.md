# Jules Issues - Compliance Module

## Environment Hurdles
- UI Tour (`test_compliance_tour`) initially failed due to Chrome startup issues and then due to the cookie bar blocking clicks on footer links. Fixed by adding a step to accept cookies using the `:not(:visible)` selector as per Odoo 19 headless constraints.
- `tools/test.py` does not seem to support `--provision-jules` despite `docs/TESTING_IN_JULES.md` mentioning it. Using `IN_JULES_VM=1` instead.

## Missing Resources
- None identified yet.

## Framework Bugs
- None identified yet.
