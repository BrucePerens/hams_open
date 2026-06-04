# JULES_ISSUES.md

## Module: database_management

### Tour Robustness Improvements
- The UI tours `db_management_bloat_tour` and `db_management_slow_query_tour` were refactored for Odoo 19 compatibility.
- Specifically, the `.o_control_panel:has(.o_breadcrumb)` selector was replaced with more direct triggers like `.o_list_renderer` to avoid race conditions.
- Added explicit navbar wait steps to ensure the SPA has fully hydrated before clicking app icons.

### Permission Issues in Jules VM
- Encountered `psycopg2.OperationalError: Permission denied` when Odoo attempted to connect to the PostgreSQL socket.
- **Resolution:** Adjusted `pg_hba.conf` to use `trust` for local connections in the test environment to bypass peer authentication issues.
- RabbitMQ failed to start due to `.erlang.cookie` permission issues. Fixed by `chown rabbitmq:rabbitmq` and `chmod 400`.

### Completed Improvements
- **Query Explain Plans:** Implemented `action_explain_query` on `database.query.stat` to generate `EXPLAIN (ANALYZE, BUFFERS)` for SELECT queries.
- **Index Recommendations:** Implemented `database.index.advisor` to identify tables that may benefit from additional indexing.
- **UI Notifications:** Added success notifications for Vacuum operations.
