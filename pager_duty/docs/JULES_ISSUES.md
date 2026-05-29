# JULES ISSUES - pager_duty

## Environment Issues
- Initial provisioning was slow but succeeded.
- Global regression tests reported 4 failures in other modules (`manual_library`, `cloudflare`, `user_websites_seo`). These appear to be pre-existing or related to the specific environment state, as `pager_duty` is not a dependency of these modules and was not part of the tracebacks.

## AI Hallucinations & Laziness
- Found `hasattr(self.env["calendar.event"], "get_current_on_duty_admin")` in `incident_ticket_adapter.py`. Repaired by removing the check as the dependency is guaranteed.
- Found `hasattr(os, "chroot")` in `pager_log_analyzer.py`. This is acceptable for a daemon that may run on different OSes, but I ensured it fails fast where appropriate.
- Found lazy `hasattr` assertion in `test_synthetic_spooler.py`. Repaired with a proper `callable(getattr(...))` check.
- Found `hasattr` in `test_incident.py` for `_daemons_started`. Replaced with `getattr(..., False)`.

## Security
- Added CWE-22 Path Traversal protection to `PagerLogAPI.search_logs` in `controllers/log_api.py`.
- Verified no unauthorized `.sudo()` calls.
- Verified all `.search()` calls have `limit` parameters.

## Multi-Tenant Awareness
- Verified `website_id` partitioning in `pager.incident` and `pager.check`.
- Verified that the NOC Dashboard respects `website_id`.

## Global Regression Failures (Documented for other sessions)
- `manual_library.tests.test_orm_logic.TestManualORMLogic.test_04_parent_deletion_restriction`: `psycopg2.errors.RestrictViolation` on `knowledge_article`.
- `cloudflare.tests.test_request_context.TestRequestContext`: `RuntimeError: object is not bound` when patching `request`.
- `user_websites_seo.tests.test_seo_ui_tour.TestSEOUI.test_01_seo_widget_tour`: Timeout waiting for `.o_list_table`.
