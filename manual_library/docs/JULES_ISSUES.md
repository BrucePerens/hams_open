# JULES ISSUES - manual_library

## Environment Verification
- **Date:** 2026-05-29
- **Environment:** Jules VM (Ubuntu 24.04)
- **Status:** Provisioned successfully.
- **Test Execution:** Standard and UI tours are functional. Headless Chrome is working correctly.

## Issues Found

### 1. Test Failure: `test_04_parent_deletion_restriction`
- **Error:** `psycopg2.errors.RestrictViolation` instead of expected `ForeignKeyViolation`.
- **Description:** The test expects `ForeignKeyViolation` from `psycopg2.errors`, but Odoo/PostgreSQL is throwing `RestrictViolation` because the `ondelete='restrict'` constraint is triggered. While `RestrictViolation` is a subclass of `ForeignKeyViolation` in some contexts, explicitly catching the wrong one or a change in how Odoo handles it might be causing the failure. Actually, Odoo's `unlink` might be catching the DB error and re-raising or it's just a mismatch in expectation.

### 2. Multi-Tenant Awareness in Admin Rule
- **Description:** `knowledge_article_admin_rule` has `[(1, '=', 1)]` as domain. While this is common for admins, it might bypass website-specific restrictions if an admin is supposed to be restricted by website. However, typically Manual Administrators are global. I will double check if this adheres to the "Multi-Tenant Awareness" mandate.

### 3. AI Hallucination Check
- None found so far in initial grep, but a deeper manual review is ongoing.
