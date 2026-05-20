# JULES_ISSUES.md - hams_test

## 2026-05-20 00:45 UTC
- **Module:** hams_test
- **Environment:** Jules VM (Ubuntu 24.04)

### Issues Encountered:

1. **Chrome Headless Startup Failures:**
   UI Tours initially failed with `Chrome headless failed to start: Failed to detect chrome devtools port after 10.0s`. This appears to be related to D-Bus and GPU sandbox issues in the Jules environment.
   - **Workaround:** Re-running the tests via `tools/test_runner.py` with `--already-provisioned` eventually succeeded once the environment stabilized, but it remains flappy.

2. **PostgreSQL Socket Location:**
   The Jules environment uses `/opt/hams/pgsock` instead of the standard `/var/run/postgresql`. This causes standard Odoo tools (like `odoo-bin` or `psql` without environment variables) to fail with `OperationalError`.
   - **Mitigation:** Always use `PGHOST=/opt/hams/pgsock` or rely on `test_runner.py` which handles this path correctly.

3. **Global Test Suite Gaps:**
   Running the full repository test suite fails due to missing dependencies in other modules (e.g., `ldap3` for `pager_duty`).
   - **Resolution:** Restricted testing to the assigned module using `-u hams_test` to avoid false positives from broken sibling modules.

4. **Linter Restrictions on Exceptions:**
   The AST Burn List linter (`check_burn_list.py`) enforces that even with `# audit-ignore-catch-all`, a catch-all `except Exception` block **must** contain a logging call.
   - **Correction:** Updated `RealTransactionCase.setUp` to include a debug log when falling back to `SUPERUSER_ID`.

5. **Pre-flight Check Blocks Startup:**
   `tools/start_odoo.py` has a strict pre-flight check that blocks startup if background daemons or periodic scripts are missing. In the Jules VM, these are not fully provisioned, making it difficult to run a persistent server for manual visual verification.
   - **Resolution:** Relied on automated UI tours which bypass these checks via `test_runner.py`.
