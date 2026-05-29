# JULES_ISSUES - database_management

## Environment Verification
- Environment provisioned successfully.
- Standard tests passed with 0 failures.

## AI Hallucinations & Laziness
- `database_management/models/db_stats.py`: `cron_check_bloat` uses `except Exception:  # audit-ignore-catch-all`. Verified that this is intentional to ensure the cron completes even if PagerDuty reporting fails. Added explanatory commentary.
- `database_management/tests/test_pg_config.py`: Removed a global monkey-patch of `shutil.which` which was a lazy testing shortcut. Replaced with proper `self.safe_patch` usage.

### Proposed Linter Rules for `check_burn_list.py`
- **Empty `except:` block detection**: Ensure all `except:` or `except Exception:` blocks without `# audit-ignore-catch-all` are flagged.
- **Flaky Tour Detection**: Flag UI tours that click action buttons but don't follow up with a wait step for DOM change or RPC resolution.
- **Sudo usage in Tests**: Even in tests, `.sudo()` should be discouraged in favor of `with_user()`.

## Multi-Tenant Awareness
- The module tracks database-wide statistics. These don't naturally have a `company_id` or `website_id`. Access is restricted to the "Database Manager" privilege group to ensure security in multi-tenant environments.

## UI Tours
- **FIXED**: Updated `db_bloat_tour.js` to use native `run: 'click'` and added a post-action wait step to ensure stability.
- **VM LIMITATION**: `expectUnloadPage: true` was found to be incompatible with Odoo `type="object"` buttons as they trigger RPCs rather than hard browser reloads. Applying it causes fatal timeouts.
- **VM LIMITATION**: `TourUtils.waitForElement` does not exist in the current `zero_sudo` JS utilities. Standard Odoo trigger polling is used instead.

## Documentation
- Updated `README.md` and `data/documentation.html` to explicitly detail the Zero-Sudo service account architecture and micro-privilege security model.
