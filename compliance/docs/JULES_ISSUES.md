# Jules Issues - Global Compliance Module

## Deep Review Findings (2026-05-29)

### AI Hallucination & Laziness
- Found and removed an architectural bypass in `compliance/tests/test_hooks.py`. The test was using a `hasattr`-style check (`"cookies_bar" in self.env["website"]._fields`) to skip the test if the field was missing. This violated the "FAIL FAST" directive. The check has been removed so that if the Odoo `website` module lacks `cookies_bar`, the test will explicitly fail, alerting developers to the architectural mismatch.

### Proposed Linter Rules for `tools/check_burn_list.py`
To prevent the patterns identified during this review, I propose the following rules be added to the global `check_burn_list.py` (or as independent AST checks):

1. **PROHIBIT_CONDITIONAL_SKIP_IN_HOOKS**:
   - *Logic*: Scan `hooks.py` files for `if 'field_name' in recordset._fields:` patterns.
   - *Reasoning*: Hooks should assume the schema defined in their `__manifest__.py` dependencies is correct. Checking for field existence masks installation errors and makes debugging hard.

2. **PROHIBIT_TEST_SKIPPING_ON_SCHEMA_MISS**:
   - *Logic*: Scan `tests/*.py` files for `unittest.SkipTest` calls that are guarded by model field existence checks.
   - *Reasoning*: Tests should fail fast if the expected architectural components are missing.

3. **MANDATE_TOUR_STABILITY_POLLING**:
   - *Logic*: Scan JS tours for steps that do not include explicit `trigger` polling for critical DOM elements before interaction.

### Security & Zero-Sudo
- Verified that `compliance/hooks.py` correctly uses `env["zero_sudo.security.utils"]._get_service_env("compliance.user_compliance_service")` for its operations. No `.sudo()` calls were found in the module.

### UI Tours
- `compliance/static/tests/tours/compliance_tour.js` has been refactored to include explicit trigger-based polling for all steps to ensure stability in dynamic environments.
- **VM LIMITATION**: `TourUtils.waitForElement` does not exist in the current `zero_sudo` JS utilities. Standard Odoo trigger polling is used instead to guarantee architectural compliance without modifying external modules.

### Multi-Tenant Awareness
- The `post_init_hook` in `compliance/hooks.py` is multi-website aware. It explicitly searches for all websites and iterates through them to enable the cookie bar. It also correctly handles page shadowing on a per-website basis.

### Documentation
- Updated `README.md` and `data/documentation.html` to remove technical jargon and ensure the guide is operable by non-technical site owners.
