# JULES_ISSUES.md

## Module: database_management

### Tour Robustness Improvements
- The UI tours `db_management_bloat_tour` and `db_management_slow_query_tour` were refactored for Odoo 19 compatibility.
- Specifically, the `.o_control_panel:has(.o_breadcrumb)` selector was replaced with more direct triggers like `.o_list_renderer` to avoid race conditions.
- Added explicit navbar wait steps to ensure the SPA has fully hydrated before clicking app icons.

### Permission Issues in Jules VM
- Encountered `psycopg2.OperationalError: Permission denied` when Odoo attempted to connect to the PostgreSQL socket at `/opt/hams/pgsock/.s.PGSQL.5432`.
- **Resolution:** Manually ran `sudo chmod 777 /opt/hams/pgsock` to allow the `odoo` user to access the socket created by the `jules` user during the test run.

### Feature Suggestions (Out of Scope)
- **Automated Tuning:** The `PgOptimizeWizard` currently requires manual input. In a future iteration, this could be linked to a system-level cron that detects hardware changes and suggests optimizations.
- **Query Explain Plans:** Add a feature to generate `EXPLAIN (ANALYZE, BUFFERS)` for slow queries directly from the APM view.
- **Index Recommendations:** Implement a "Missing Index" advisor based on sequential scan statistics vs table size.
