# Jules VM Environment Testing Issues

## 1. Provisioning Issues
Running `./tools/test.py --provision-jules` (or `IN_JULES_VM=1 python3 tools/test.py --provision-jules`) resulted in the following issues:

- When running against all modules, the AST Burn List Linter halted execution due to burn list violations in the `hams_helpdesk` module:
  - `hams_helpdesk/static/tests/tours/helpdesk_operator_tour.js`
    - Line 10: FRAGILE TOUR TRIGGER: Odoo 19 UI shifted. Do not use '.o_app', '.nav-link', '.o_menu_brand', or 'h1:contains' in tour triggers. Use structure-agnostic selectors like '[data-menu-xmlid=...]' or '*:contains'.
    - Line 12: FRAGILE TOUR TRIGGER: Odoo 19 UI shifted. Do not use '.o_app', '.nav-link', '.o_menu_brand', or 'h1:contains' in tour triggers. Use structure-agnostic selectors like '[data-menu-xmlid=...]' or '*:contains'.
- When targeting `user_websites` directly with `IN_JULES_VM=1 python3 tools/test.py -u user_websites --provision-jules`, the provisioning fails with the error:
  - `❌ ERROR: PostgreSQL initdb not found.`

## 2. Standard Test Execution Issues
Running standard tests on the module using `./tools/test.py -u user_websites` resulted in the following issues:

- The test runner crashed while trying to drop and rebuild the database schema (`hams_test`).
- A Python traceback occurred with `FileNotFoundError: [Errno 2] No such file or directory: 'psql'` in `subprocess.run` inside `tools/test.py` at line 495 because the `psql` command is not installed or available in the system PATH.
