# User Stories: Database Management

## Performance Monitoring
### Monitor Table and Index Health
**As a** Database Administrator
**I want** to see real-time statistics on table bloat `COMM_db_table_stat_init` and index usage `COMM_db_index_stats` (backed by the underlying index-stat view `COMM_db_index_stat_init`) and replication status `COMM_db_replication_stats` (backed by `COMM_db_replication_stat_init`), using whichever of `vacuumdb`/`psql` is actually installed on the server `COMM_db_table_stat_get_executable`
**So that** I can identify tables that need vacuuming and indexes that are unused or oversized.

### Track Slow Queries
**As a** System Engineer
**I want** to see a list of the most time-consuming SQL queries `COMM_db_slow_queries` (backed by the `pg_stat_statements`-driven view `COMM_db_query_stat_init`, installable directly from the UI `COMM_db_install_extension` and resettable once I've acted on a finding `COMM_db_reset_stats`) and generate deep-dive explain plans `COMM_db_explain_query`, closing the wizard when I'm done reading one `COMM_pg_explain_wizard_close`
**So that** I can optimize the application code or add missing indexes.

### Proactive Index Advice
**As a** Database Administrator
**I want** the system to suggest tables that might benefit from additional indexing `COMM_db_index_advisor` (backed by `COMM_db_index_advisor_init`)
**So that** I can proactively improve query performance for large tables.

## Incident Remediation
### Manually Reclaim Disk Space
**As a** System Administrator
**I want** to trigger a `VACUUM ANALYZE` `COMM_vacuum_analyze` on specific bloated tables from the Odoo UI
**So that** I can reclaim space and update table statistics without needing direct SSH access to the database server.

### Terminate Runaway Sessions
**As a** Database Administrator
**I want** to view active database sessions `COMM_db_active_sessions` (backed by `COMM_db_activity_init`) and terminate specific backends `COMM_db_terminate_backend`
**So that** I can stop long-running or locked queries that are impacting system performance.

## System Optimization
### Tune PostgreSQL Parameters
**As a** DevOps Engineer
**I want** a wizard `COMM_pg_optimize_wizard`, audit views `COMM_db_settings_audit` (backed by `COMM_db_pg_setting_init`), and specialized dashboards `COMM_test_pg_config_views` that suggest PostgreSQL settings based on my server's RAM and CPU
**So that** I can maximize the performance of the database engine for my specific hardware.

### Configure High Availability
**As a** Site Reliability Engineer
**I want** to generate configuration templates for Patroni and PgBouncer `COMM_pg_ha_wizard`, with my own inputs validated before anything is generated `COMM_pg_ha_wizard_validate_inputs` and using whichever `etcd`/`patroni`/`pgbouncer` binaries are actually available `COMM_pg_ha_wizard_get_executable`
**So that** I can quickly set up a resilient, failover-ready database cluster.

## Automated Governance
### Automated Bloat Alerts
**As a** Site Reliability Engineer
**I want** the system to automatically notify PagerDuty when table bloat exceeds safe thresholds `COMM_bloat_alert_synergy`
**So that** I am alerted to performance degradation before it becomes a critical outage.

## Verification
### Ensure DBA Tools are Reliable
**As a** Developer
**I want** automated tests for the DBA cron jobs `COMM_test_dba_cron`, stat views `COMM_test_dba_view`, configuration dashboards `COMM_test_pg_config_views`, and security prefetching `COMM_db_security_prefetch`
**So that** I can be confident that the management tools are reporting accurate data and functioning correctly after every update.

### Seamless Documentation Access
**As a** New User
**I want** the module's documentation to be automatically available in the Odoo Knowledge base upon installation `COMM_db_doc_injection`
**So that** I can learn how to use the DBA tools without searching external repositories.

## Anchor reference

One entry per anchor named in this document, read from the module code (`models/db_stats.py`, `models/pg_config.py`, `data/`, `views/`, `tests/`). Entries share their text with the other documents of the module, so the same anchor is described the same way everywhere.

### `COMM_db_table_stat_init` [@ANCHOR: COMM_db_table_stat_init]

`DatabaseTableStat.init(self)`, called by Odoo on module install and upgrade. It drops any existing view named `database_table_stat` (`drop_view_if_exists`) and runs `CREATE OR REPLACE VIEW` over `pg_stat_user_tables` joined to `pg_statio_user_tables` on `relid`. The view supplies `id` (`row_number() OVER ()`, so an id is not stable between refreshes), the constant name `'SQL View'`, and the columns of `COMM_db_security_prefetch` with the dead-percent and cache-hit divisions guarded against zero. Takes no arguments, returns nothing, and raises a database error if the catalog views are not readable. It runs in the install transaction, takes a brief lock for the view replacement and has no other side effects. Test: `test_04_views` (`COMM_test_dba_view`).

### `COMM_db_index_stats` [@ANCHOR: COMM_db_index_stats]

The model `database.index.stat`, the index-health list, backed by the view of `COMM_db_index_stat_init`; global and read-only. Default order `idx_scan asc`, so unused indexes come first.

| Field | Type | Meaning |
|---|---|---|
| `table_name` | Char | Table the index belongs to. |
| `index_name` | Char | Index name. |
| `idx_scan` | Integer | Number of scans that used it since statistics were last reset. |
| `index_size_kb` | Float | Size in kilobytes. |

Unique indexes are excluded because they enforce constraints and cannot simply be dropped. Access: Database Manager group, read only. Tests: `test_04_views` and `test_03_db_index_stats` (which in fact exercises `cron_check_bloat`; to-do: point it at this model).

### `COMM_db_index_stat_init` [@ANCHOR: COMM_db_index_stat_init]

`DatabaseIndexStat.init(self)`. Drops and recreates the view `database_index_stat` from `pg_stat_user_indexes` joined with `pg_index` on `indexrelid`, keeping only rows with `indisunique IS FALSE`. Columns: `id` (`row_number() OVER ()`), constant name `'SQL View'`, `table_name` (`relname`), `index_name` (`indexrelname`), `idx_scan`, and `index_size_kb` (`pg_relation_size(indexrelid) / 1024.0`). No arguments and no return value; it runs on install and upgrade and only reads catalog statistics. Test: `test_04_views`.

### `COMM_db_replication_stats` [@ANCHOR: COMM_db_replication_stats]

The model `database.replication.stat`, the replication monitor, backed by the view of `COMM_db_replication_stat_init` over `pg_stat_replication`. It is empty on a server with no standby connected. Read-only, global to the cluster, readable by the Database Manager group only.

| Field | Type | Meaning |
|---|---|---|
| `usename`, `application_name`, `client_addr` | Char | Standby's role, name and address. |
| `state`, `sync_state` | Char | Streaming state and synchronous status. |
| `sent_lsn`, `write_lsn`, `flush_lsn`, `replay_lsn` | Char | Write-ahead-log positions cast to text. |
| `write_lag`, `flush_lag`, `replay_lag` | Char | Lag intervals cast to text. |
| `sync_priority` | Integer | Priority for synchronous replication. |

Tests: `test_04_views`, `COMM_test_dba_cron`.

### `COMM_db_replication_stat_init` [@ANCHOR: COMM_db_replication_stat_init]

`DatabaseReplicationStat.init(self)`. Drops and recreates the view `database_replication_stat` selecting from `pg_stat_replication`: `id` from `row_number() OVER ()`, constant name `'SQL View'`, user, application, client address, state, the four LSN columns and three lag columns cast to `text` (so the Odoo model can hold them in Char fields), `sync_priority` and `sync_state`. No arguments, no return value, read-only on the catalog; runs at install and upgrade. Test: `test_04_views`.

### `COMM_db_table_stat_get_executable` [@ANCHOR: COMM_db_table_stat_get_executable]

`DatabaseTableStat._get_executable(self, cmd_name)` resolves the full path of a command-line tool for the vacuum action. It calls `zero_sudo.security.utils._ensure_executable(cmd_name, svc_xml_id="database_management.user_database_management_service", pkg_name=...)`. The package name is `postgresql-client` when `cmd_name` is `vacuumdb`, otherwise the command name itself. The helper uses the binary found on `PATH` or, if none, asks `binary_downloader` to install it as the module's service account. Returns the path as a string; errors from the helper (the binary cannot be found or installed) propagate to the caller. No `.sudo()` is used. Test: `test_01_vacuum_analyze` patches it.

### `COMM_db_slow_queries` [@ANCHOR: COMM_db_slow_queries]

The model `database.query.stat`, the slow-query list (APM). Read-only, backed by the view of `COMM_db_query_stat_init` over `pg_stat_statements` for the current database; global across companies. Default order `total_time desc`.

| Field | Type | Meaning |
|---|---|---|
| `query` | Text | Normalised SQL text. |
| `calls` | Integer | Execution count. |
| `total_time` | Float | Total execution time, milliseconds (`total_exec_time`). |
| `mean_time` | Float | Mean execution time, milliseconds. |

Without the extension the view holds one placeholder row whose `query` explains the missing setup. **Buttons:** `action_install_extension` (`COMM_db_install_extension`), `action_reset_stats` (`COMM_db_reset_stats`) and `action_explain_query` (`COMM_db_explain_query`). Access: Database Manager group, read only. Tests: `test_04_views`, `test_06_query_stats_ops`.

### `COMM_db_query_stat_init` [@ANCHOR: COMM_db_query_stat_init]

`DatabaseQueryStat.init(self)`. Drops the view `database_query_stat`, then checks in a savepoint that the `pg_stat_statements` extension exists and appears in `shared_preload_libraries`; the check avoids an error log line. If so it creates the view over `pg_stat_statements` limited to the current database (`id` from `row_number()`, constant name `'SQL View'`, `query`, `calls`, `total_exec_time`, `mean_exec_time`); if not, it creates a one-row placeholder view whose text says the extension is not installed or not preloaded. A failing check is logged and degrades to the placeholder. It also (re)creates the SQL function `dba_explain_query(q text)`, which sets `default_transaction_read_only = on` for the transaction and returns the `EXPLAIN (ANALYZE, BUFFERS)` plan of `q` as text. Takes no arguments and returns nothing. Test: `test_04_views`.

### `COMM_db_install_extension` [@ANCHOR: COMM_db_install_extension]

`DatabaseQueryStat.action_install_extension(self)` (`@public_rpc`), the "Install pg_stat_statements" button. Executes `CREATE EXTENSION IF NOT EXISTS pg_stat_statements` with the caller's cursor, then calls `init()` again to rebuild the view. **Returns** a client action showing the notification "Installation Attempted" asking the user to refresh. **Errors:** any exception is logged and re-raised as `UserError` with three numbered steps (set `shared_preload_libraries = 'pg_stat_statements'`, restart PostgreSQL, make sure the database user may `CREATE EXTENSION`) and the technical detail. It cannot make PostgreSQL preload the library: that needs a server restart outside this module. Idempotent. Test: `test_05_action_install_extension_happy_path`.

### `COMM_db_reset_stats` [@ANCHOR: COMM_db_reset_stats]

`DatabaseQueryStat.action_reset_stats(self)` (`@api.private`), clears the recorded query statistics. It switches to the `database_management.user_database_management_service` environment and runs `SELECT pg_stat_statements_reset()` on that cursor. **Returns** a "Stats Reset" notification action. **Errors:** any exception is logged and raised as `UserError("Could not reset statistics: ...")`, for example when the extension is missing. The reset is global to the server's statistics (not only this database's rows) and cannot be undone. Test: `test_06_query_stats_ops`.

### `COMM_db_explain_query` [@ANCHOR: COMM_db_explain_query]

`DatabaseQueryStat.action_explain_query(self)` (`@public_rpc`), the Explain button on one slow-query row (`ensure_one`). It strips the stored query text and refuses anything that does not start with `SELECT` or `WITH` (case-insensitive), so a data-changing statement is never run. It then calls `SELECT dba_explain_query(%s)` as the module service account with the text as a bound parameter. The SQL function turns on read-only transactions before executing `EXPLAIN (ANALYZE, BUFFERS)`, so the query is really executed once but cannot write. The plan is stored in a new `pg.explain.wizard` record and the method returns an `ir.actions.act_window` opening it as a dialog titled "Query Explain Plan". **Errors:** every failure (including the refusal) is logged and raised as `UserError("Could not generate explain plan: ...")`. The query can be expensive. Tests: `test_07_explain_query_security`, `test_tdd_db_stats_transaction`, `test_tdd_db_stats_semicolon`.

### `COMM_pg_explain_wizard_close` [@ANCHOR: COMM_pg_explain_wizard_close]

`PgExplainWizard.action_close(self)` (`@public_rpc`) on the transient model `pg.explain.wizard`, which shows a query and its plan (read-only fields `query` and `explain_plan`, both Text, created by `COMM_db_explain_query`). The method takes no arguments and returns `{"type": "ir.actions.act_window_close"}`, which closes the dialog and returns the user to the list. No side effects; the wizard record is discarded later by Odoo's transient-model vacuum. Access rows give the Database Manager group full rights on the wizard. Test: `test_07b_explain_wizard_close`.

### `COMM_db_index_advisor` [@ANCHOR: COMM_db_index_advisor]

The model `database.index.advisor`, which lists tables that may need an index; read-only, backed by `COMM_db_index_advisor_init`, default order `seq_scan desc`. A table appears when it has more than 100 sequential scans and is larger than 10 MB. It only names candidate tables; it does not propose index definitions or columns.

| Field | Type | Meaning |
|---|---|---|
| `table_name` | Char | Table name. |
| `seq_scan` | Integer | Sequential scans since statistics reset. |
| `seq_tup_read` | Integer | Rows read by those scans. |
| `idx_scan` | Integer | Index scans, for comparison. |
| `table_size_mb` | Float | Total relation size in MB. |

Access: Database Manager group, read only. Tests: `test_04_views` and `COMM_test_dba_cron`.

### `COMM_db_index_advisor_init` [@ANCHOR: COMM_db_index_advisor_init]

`DatabaseIndexAdvisor.init(self)`. Drops and recreates the view `database_index_advisor` over `pg_stat_user_tables` with `id` set to `relid`, `table_name`, `seq_scan`, `seq_tup_read`, `idx_scan` and `table_size_mb` (`pg_total_relation_size(relid)` in MB), filtered by `seq_scan > 100 AND pg_total_relation_size(relid) > 10 * 1024 * 1024`. The thresholds are fixed in the SQL and not configurable. No arguments and no return value; runs on install and upgrade. Test: `test_04_views`.

### `COMM_vacuum_analyze` [@ANCHOR: COMM_vacuum_analyze]

`DatabaseTableStat.action_vacuum_analyze(self)` (`@public_rpc`), the "Vacuum Analyze" button on the table-statistics list. It runs `vacuumdb -z -t <table> ... <dbname>` once for all selected rows that have a `table_name`, in a subprocess so the vacuum is outside the ORM transaction (a vacuum cannot run inside one).

| Input | Detail |
|---|---|
| `self` | One or more `database.table.stat` records; rows without a table name are ignored. |
| environment | Built minimally (ADR 0044): `PATH`, `PGPORT` (default 5432), `PGUSER` (default `odoo`) and `PGHOST` only if set. No password is passed: with `PGHOST` unset `vacuumdb` uses the local socket and peer authentication. |

**Returns:** `True` (also when nothing was selected). **Errors:** `UserError` with the stderr text when `vacuumdb` exits non-zero, "timed out" after 600 seconds, or on `OSError`. **Safety:** `shell=False`, table names go as separate argv items, the binary comes from `COMM_db_table_stat_get_executable`. Blocking for up to ten minutes, so it holds a web worker. Idempotent and safe to repeat. Tests: `test_01_vacuum_analyze`, `test_01a_vacuum_uses_the_local_socket_and_never_a_password`, `test_01b_vacuum_analyze_failures`.

### `COMM_db_active_sessions` [@ANCHOR: COMM_db_active_sessions]

The model `database.activity`, the list of other PostgreSQL sessions in this database, backed by the view of `COMM_db_activity_init`; global and read-only. Default order `duration desc`.

| Field | Type | Meaning |
|---|---|---|
| `pid` | Integer | Backend process id (also the record id). |
| `usename` | Char | Database user. |
| `state` | Char | `active`, `idle`, `idle in transaction`, and so on. |
| `query` | Text | Current or most recent query text. |
| `duration` | Float | Seconds since `query_start`. |

The Odoo session itself is excluded. Its button `action_terminate_backend` is `COMM_db_terminate_backend`. Only the Database Manager group can read it. Tests: `test_04_views`, `test_03_terminate_backend`.

### `COMM_db_activity_init` [@ANCHOR: COMM_db_activity_init]

`DatabaseActivity.init(self)`. Drops and recreates the view `database_activity` over `pg_stat_activity`, keeping rows with `datname = current_database()` and `pid <> pg_backend_pid()`, so the viewer never lists (or terminates) its own connection. Columns: constant name `'SQL View'`, `id` and `pid` both equal to the process id, `usename`, `state`, `query`, and `duration` as `EXTRACT(EPOCH FROM now() - query_start)` seconds (null when there is no start time). No arguments, no return value; runs at install and upgrade. Note that PostgreSQL shows query text only for sessions the Odoo database user may see. Test: `test_04_views`.

### `COMM_db_terminate_backend` [@ANCHOR: COMM_db_terminate_backend]

`DatabaseActivity.action_terminate_backend(self)` (`@public_rpc`), the "Terminate Session" button. It collects the `pid` of every selected row, switches to the `user_database_management_service` environment and runs one statement, `SELECT pg_terminate_backend(pid) FROM unnest(%s) AS pid`, with the pid list as a bound array (one round trip for any number of sessions). **Returns** `True`, also when the list is empty. Errors from the database propagate. The service account is a PostgreSQL role with enough privilege to signal the target backends; a pid that no longer exists or belongs to a superuser is simply not terminated (`pg_terminate_backend` returns false and the result is not inspected). A terminated client loses its transaction. Not undoable. Tests: `test_03_terminate_backend`.

### `COMM_pg_optimize_wizard` [@ANCHOR: COMM_pg_optimize_wizard]

`PgOptimizeWizard.action_apply_optimizations(self)` (`@public_rpc`) on the transient model `pg.optimize.wizard`, which computes PostgreSQL tuning values from the hardware and writes them with `ALTER SYSTEM`.

| Field | Type | Meaning |
|---|---|---|
| `ram_gb` | Integer | Server RAM in GB, default 8, must be above 0. |
| `cpu_cores` | Integer | Cores, default 4, above 0. |
| `storage_type` | Selection | `ssd` (default) or `hdd`. |
| `max_connections` | Integer | Default 200, above 0. |

**Formulas:** `shared_buffers` 25% of RAM; `effective_cache_size` 75%; `maintenance_work_mem` 5% capped at 1024 MB; `work_mem` = 25% of RAM / `max_connections`, at least 4 MB; `random_page_cost` 1.1 (ssd) or 4.0; `max_worker_processes` and `max_parallel_workers` = cores; `max_parallel_workers_per_gather` = max(2, cores/2). **Behaviour:** values go through `psycopg2.sql` (`Identifier` and `Literal`) on a separate registry cursor, followed by `pg_reload_conf()`. **Returns** a sticky warning notification that `shared_buffers` and `max_connections` need a PostgreSQL restart. **Errors:** `UserError` for a non-positive input. Writes `postgresql.auto.conf`, a persistent server-wide change; the database user must be a superuser. Tests: `test_01_optimization_wizard`, `test_01b_optimization_wizard_errors`.

### `COMM_db_settings_audit` [@ANCHOR: COMM_db_settings_audit]

The model `database.pg.setting`, the PostgreSQL configuration viewer (menu: PostgreSQL Configuration), backed by the view of `COMM_db_pg_setting_init` over `pg_settings`; global, read-only, default order `category asc, name asc`.

| Field | Type | Meaning |
|---|---|---|
| `name` | Char | Parameter name. |
| `setting` | Char | Current value. |
| `unit` | Char | Unit, if any. |
| `category` | Char | PostgreSQL's settings category. |
| `short_desc` | Char | One-line description. |
| `context` | Char | When it can change (`postmaster` means restart). |
| `pending_restart` | Boolean | True when a changed value waits for a restart. |

It lets an administrator audit values before and after running the optimization wizard. Only the Database Manager group can read it. Tests: `test_03_views` (`COMM_test_pg_config_views`), `test_01_security`.

### `COMM_db_pg_setting_init` [@ANCHOR: COMM_db_pg_setting_init]

`DatabasePgSetting.init(self)`. Drops and recreates the view `database_pg_setting` selecting `name`, `setting`, `unit`, `category`, `short_desc`, `context` and `pending_restart` from `pg_settings`, with `id` from `row_number() OVER (ORDER BY name)`. No arguments, no return value, only reads the catalog; runs at install and upgrade. Test: `test_03_views` in `tests/test_pg_config.py`.

### `COMM_test_pg_config_views` [@ANCHOR: COMM_test_pg_config_views]

Test id for the PostgreSQL configuration views. `test_03_views` in `tests/test_pg_config.py` calls `get_view` for the `database.pg.setting` list (expects the field `setting`), the `pg.optimize.wizard` form (`ram_gb`) and the `pg.ha.wizard` form (`primary_ip`), proving they compile. The settings list and the two wizard forms in `views/pg_config_views.xml` cite this anchor as their audit-ignore reason. The same test also cites `COMM_db_settings_audit` and `COMM_db_pg_setting_init`. Failure means a view or a field name changed.

### `COMM_pg_ha_wizard` [@ANCHOR: COMM_pg_ha_wizard]

`PgHaWizard.action_generate(self)` (`@public_rpc`) on the transient model `pg.ha.wizard`, the High Availability orchestrator. It only produces text for the operator to copy; it deploys nothing.

| Field | Type | Meaning |
|---|---|---|
| `cluster_name` | Char | Patroni scope, default `hams_cluster`. |
| `primary_ip`, `secondary_ip` | Char | Node addresses, defaults 10.0.0.1 and 10.0.0.2. |
| `etcd_hosts` | Char | Comma-separated `host:port`, default `etcd:2379`. |
| `replication_user`, `superuser_user` | Char | Defaults `replicator`, `postgres`. |
| `replication_pass` | Char | Default `secrets.token_urlsafe(24)`; also used for the superuser entry. |
| `state` | Selection | `input` then `generated`. |
| `patroni_primary`, `patroni_secondary`, `pgbouncer_ini` | Text | Outputs, read-only. |

**Steps:** `_validate_inputs()` (`COMM_pg_ha_wizard_validate_inputs`), then `_get_executable` for `patroni`, `pgbouncer` and `etcd`, then it fills the three outputs (node1 includes an `initdb` bootstrap section; PgBouncer listens on 6432 in transaction pool mode) and sets `state = generated`. **Returns** an `ir.actions.act_window` reopening the wizard. **Errors:** `UserError` from validation or a missing binary. The outputs contain the replication password in clear text, so they are shown only to Database Managers. Tests: `test_02_ha_wizard`, `test_02a_ha_wizard_password_not_predictable`, the `test_tdd_pg_config_yaml_injection*` set.

### `COMM_pg_ha_wizard_validate_inputs` [@ANCHOR: COMM_pg_ha_wizard_validate_inputs]

`PgHaWizard._validate_inputs(self)`, called first by `COMM_pg_ha_wizard`. It raises `UserError` unless: the primary and secondary IPs parse with `ipaddress.ip_address`; the replication password has at least 8 characters and is free of YAML-breaking content (control characters, `:` followed by whitespace or the end, whitespace followed by `#`, trailing whitespace, or a leading character from `` `@%&*!|>'",[{]}# ``); `cluster_name`, `superuser_user` and `replication_user` match `^[a-zA-Z0-9_]+\Z` (so a trailing newline is refused); and `etcd_hosts` is comma-separated `host:port` pairs matching `^[a-zA-Z0-9_.-]+:[0-9]+(,...)*\Z`. The checks exist because these values are put unescaped into generated YAML. Returns nothing; no side effects. Tests: `test_02d_ha_wizard_validation_errors`, `test_tdd_pg_config_*` including the trailing-newline and password cases.

### `COMM_pg_ha_wizard_get_executable` [@ANCHOR: COMM_pg_ha_wizard_get_executable]

`PgHaWizard._get_executable(self, cmd_name)` resolves the full path of `patroni`, `pgbouncer` or `etcd` for the High Availability wizard. It calls `zero_sudo.security.utils._ensure_executable(cmd_name, svc_xml_id="database_management.user_database_management_service", pkg_name=...)` where the package name is the command name. A binary found on `PATH` is used; otherwise `binary_downloader` is asked to install it, as the service account (the test `test_02c_etcd_auto_download` covers etcd). Returns the path string. Errors from the helper propagate, so generating the configuration fails when a tool is neither present nor installable. The wizard only checks presence; it does not run these tools. Tests: `test_02c_etcd_auto_download`, `test_02b_ha_wizard_missing_binaries`.

### `COMM_bloat_alert_synergy` [@ANCHOR: COMM_bloat_alert_synergy]

`DatabaseTableStat.cron_check_bloat(self)` (`@api.private`, `@api.model`), run once a day by the cron "DBA: Check Autovacuum Bloat" as the service account. It searches `database.table.stat` for tables with `dead_percent > 20` and `dead_tuples > 10000` (at most 1000 rows). If any exist it opens one incident through `pager.incident.report_incident` (the project's own `pager_duty` module) with source "DBA Autovacuum Monitor", severity `medium` and a description listing each table with its dead percentage and asking for a manual Vacuum Analyze. **Returns** nothing. **Errors:** `AccessError` and `UserError` from the pager service are logged as warnings and swallowed, so a misconfigured pager never fails the cron. Elevation uses `_get_service_env("pager_duty.user_pager_service_internal")`. Repeated runs open a new incident each day while the bloat persists. Tests: `test_02b_bloat_alert_synergy` and `COMM_test_dba_cron`.

### `COMM_test_dba_cron` [@ANCHOR: COMM_test_dba_cron]

Test id for the scheduled bloat check and the stat models. `test_02_bloat_cron` patches `_get_service_env` with a mock and triggers the cron record `database_management.cron_check_bloat` (job "DBA: Check Autovacuum Bloat", model `database.table.stat`, run as `user_database_management_service`, daily, code `model.cron_check_bloat()`, with the manager group named on the job so Odoo allows the action). The same anchor is cited by `data/cron.xml` as the audit-ignore justification and on the replication and index-advisor models. Failure means the cron record or `cron_check_bloat` no longer runs. See `COMM_bloat_alert_synergy`.

### `COMM_test_dba_view` [@ANCHOR: COMM_test_dba_view]

Test id for the statistics views. `test_04_views` in `tests/test_db_management.py` calls `get_view(view_type="list")` on `database.table.stat`, `database.query.stat`, `database.activity`, `database.index.stat`, `database.replication.stat` and `database.index.advisor`, and `get_view(view_type="form")` on `pg.explain.wizard`, asserting that each compiled arch contains a known field. That proves the views in `views/db_stats_views.xml` compile against the models; they cite this anchor as their audit-ignore reason. It does not read rows. The model `database.table.stat` (`COMM_db_security_prefetch`) cites it too. Failure means a view or a field name changed.

### `COMM_db_security_prefetch` [@ANCHOR: COMM_db_security_prefetch]

The model `database.table.stat` (`models/db_stats.py`), the table-statistics model of the module. It is read-only and has no table of its own (`_auto = False`): it is backed by the SQL view created by `COMM_db_table_stat_init`, so it is logically global (the whole current database, not one Odoo company).

| Field | Type | Meaning |
|---|---|---|
| `table_name` | Char | Relation name from `pg_stat_user_tables`. |
| `live_tuples`, `dead_tuples` | Integer | `n_live_tup` and `n_dead_tup`. |
| `dead_percent` | Float | dead / (live + dead) x 100; 0 when the table is empty. The default sort is `dead_percent desc`. |
| `total_size_mb` | Float | `pg_total_relation_size` in MB, including indexes and TOAST. |
| `cache_hit_percent` | Float | Heap block hits / (hits + reads) x 100, from `pg_statio_user_tables`. |

**Security:** only `group_database_management_manager` holds an access row, read-only (`perm_write`, `perm_create` and `perm_unlink` are 0); every other persona, including portal and public, gets `AccessError`. **Methods:** `action_vacuum_analyze` and `cron_check_bloat`. **Tests:** `test_01_security` in `tests/test_db_security.py` and `COMM_test_dba_view`.

### `COMM_db_doc_injection` [@ANCHOR: COMM_db_doc_injection]

Documentation bootstrap. `__manifest__.py` lists `knowledge_docs` with one entry, "Database Management Guide" (`data/documentation.html`, icon, category `workspace`). When the Knowledge module is installed, `zero_sudo`'s knowledge-doc bootstrap reads that key and creates or updates the article, so this module has no code of its own for it and uses no elevated privilege. The HTML file itself carries the other anchors of the module as comments, one section per feature (bloat alerts, vacuum, index advisor, index stats, slow queries and explain, active sessions, replication, tuning, high availability). Test: `test_08_doc_injection`, which only checks that the file opens (to-do: assert the article exists once Knowledge is present).
