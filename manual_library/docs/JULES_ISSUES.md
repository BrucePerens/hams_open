# Jules VM Issues & Architectural Decisions - manual_library

## 1. UI Tour Fixes
- **manual_toc_tour:** Fixed a potential hang in the tour by removing unnecessary empty `run: function() {}` blocks which can sometimes cause issues in the Odoo 19 tour executor when combined with complex OWL interactions. Verified that the tour now passes consistently in the headless environment.
- **Tour Best Practices:** Strictly adhered to the ban on `:contains` pseudo-selectors in tour triggers as enforced by Odoo 19 and the Jules VM linters.

## 2. Multi-Tenant Isolation
- Verified that all controllers (`ManualLibraryController`) and record rules (`ir.rule`) strictly enforce `website_id` and `company_id` isolation.
- Articles are filtered to the current website context in both the sidebar navigation and search results.

## 3. Security Audit
- Confirmed that `.sudo()` is not used for data access. Feedback increments are performed via a dedicated Service Account (`manual_library.user_manual_library_service_account`) using raw SQL for atomicity and security.
- Record rules correctly handle public, portal, and internal user personas, ensuring unpublished or private articles remain inaccessible.

## 4. Test Output Verbosity
- Added `[!] DIAGNOSTIC FOR AI:` messages to critical assertions in `test_access_rights.py` to aid in future autonomous troubleshooting.
