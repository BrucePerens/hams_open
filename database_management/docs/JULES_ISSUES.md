# JULES_ISSUES - database_management

## Session: 2026-05-29

### Environment Verification
- [X] VM Provisioning: SUCCESS
- [X] Basic Test Run: SUCCESS (25 tests, 0 failures)

### AI Hallucination & Laziness
- **FIXED**: Refactored `hasattr(call[0][0], "as_string")` in `database_management/tests/test_pg_config.py` to use `isinstance(call[0][0], sql.Composable)`.
- **FIXED**: Replaced a lazy global monkey-patch of `shutil.which` in `test_db_management.py` with proper `self.safe_patch` (already part of the base module but verified).
- **OBSERVATION**: `except Exception:  # audit-ignore-catch-all` in `database_management/models/db_stats.py` is intentional to prevent cron failure on PagerDuty integration errors.

### Proposed Linter Rules for `check_burn_list.py`
- **Empty `except:` block detection**: Flag all `except:` or `except Exception:` blocks without `# audit-ignore-catch-all`.
- **Flaky Tour Detection**: Flag UI tours that click action buttons but don't follow up with a wait step for DOM change or RPC resolution (e.g., `body:not(:has(.o_loading))`).
- **Sudo usage in Tests**: Discourage `.sudo()` in favor of `with_user()`.

### Zero-Sudo & Micro-Privilege
- Verified `database_management` models use `user_database_management_service` service account.
- Confirmed no `.sudo()` calls in business logic.

### Multi-Tenant Awareness
- Verified database statistics are correctly restricted to the `Database Manager` privilege group.

### UI Tours
- **FIXED**: Updated `db_bloat_tour.js` and `db_slow_query_tour.js` to wait for RPC resolution/DOM rendering after actions.
- **VM LIMITATION**: `expectUnloadPage: true` causes timeouts with Odoo AJAX actions.
- **VM LIMITATION**: `TourUtils.waitForElement` is not available; used standard Odoo trigger polling.

### Documentation
- **FIXED**: Updated `README.md` and `data/documentation.html` to detail the Zero-Sudo architecture and micro-privilege model.
