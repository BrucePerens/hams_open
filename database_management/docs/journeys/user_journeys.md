# User Journeys: Database Management

## Journey 1: Identifying and Resolving Table Bloat
**Context:** A system administrator notices a slow-down in Odoo and suspects database performance issues.

1. **Dashboard Review:** The administrator opens the "Table Statistics" view `COMM_db_index_stats` and sorts by "Dead %".
2. **Detection:** They identify the `mail_message` table as having 45% bloat.
3. **Remediation:** They select the table and click the **Vacuum Analyze** button `COMM_vacuum_analyze`.
4. **Verification:** The system executes the command via `vacuumdb` in a background process to avoid ORM timeouts.
5. **Observation:** The administrator refreshes the view to see the "Dead %" drop and performance improve.

## Journey 2: Optimizing Database for New Hardware
**Context:** The Odoo instance has been migrated to a larger server with 64GB RAM and 16 CPU cores.

1. **Audit:** The DevOps engineer reviews the current settings in the **PostgreSQL Configuration** view `COMM_db_settings_audit`.

2. **Wizard Activation:** The engineer launches the **PostgreSQL Optimization Wizard** `COMM_pg_optimize_wizard`.
2. **Configuration:** They enter the new RAM and CPU specifications and select "SSD" storage.
3. **Application:** They click "Apply Optimizations", which executes `ALTER SYSTEM` commands for `shared_buffers`, `work_mem`, and other key parameters.
4. **Reload:** The wizard triggers a configuration reload (`pg_reload_conf()`).
5. **Finalization:** The engineer follows the UI notification to restart the PostgreSQL service to apply the core memory changes.

## Journey 3: Scaling with High Availability
**Context:** To ensure 99.9% uptime, the SRE needs to move from a single DB node to a clustered setup.

1. **HA Setup:** The SRE opens the **HA Failover Wizard** `COMM_pg_ha_wizard`.
2. **Parameters:** They input the IP addresses for the primary and secondary nodes.
3. **Generation:** They click "Generate Configuration".
4. **Provisioning:** The system checks for the `etcd`, `patroni`, and `pgbouncer` binaries. If `etcd` is missing, it is automatically downloaded.
5. **Deployment:** The SRE copies the generated YAML and INI templates to their respective nodes to initialize the cluster.

## Journey 4: Terminating Runaway Sessions
**Context:** A developer accidentally ran a query without a proper WHERE clause, locking a critical table.

1. **Activity Monitoring:** The DBA opens the "Active Sessions" dashboard `COMM_db_active_sessions`.
2. **Identification:** They find the PID of the query that has been running for 30 minutes.
3. **Action:** They click the **Terminate Session** button `COMM_db_terminate_backend`.
4. **Verification:** The system sends a `pg_terminate_backend` signal and the session disappears from the list.

## Journey 5: Investigating Slow Queries
**Context:** Users report that the "Order Entry" screen takes 5 seconds to load.

1. **Slow Query Log:** The System Engineer navigates to the **Slow Query Tracking** view `COMM_db_slow_queries`.
2. **Identification:** They find an `UPDATE` query that is called 5,000 times an hour with a high mean execution time.
3. **Analysis:** They use the query text to locate the offending Odoo method and implement batch processing.
4. **Verification:** They clear the stats and monitor the view to ensure the query is no longer a bottleneck.

## Journey 6: Proactive Incident Management
**Context:** An unoptimized module is creating many dead tuples during a mass import.

1. **Background Monitoring:** The DBA Autovacuum Monitor cron `COMM_test_dba_cron` runs every hour. (The [module README](../../README.md) says the alerting cron runs daily; where they differ, treat the README as the later statement.)
2. **Threshold Breach:** The cron detects bloat exceeding 20% on several critical tables. (Per the README, the thresholds are more than 20% dead tuples and more than 10,000 dead tuples in the table.)
3. **Alerting:** The system automatically triggers a PagerDuty incident `COMM_bloat_alert_synergy` (this project's own `pager_duty` module; per the README, one incident of severity medium listing every table over the thresholds).
4. **Response:** The on-call SRE receives the alert and navigates to Odoo to perform a manual Vacuum or investigate the source of the bloat.

## Anchor reference

One entry per anchor named in this document, read from the module code (`models/db_stats.py`, `models/pg_config.py`, `data/`, `views/`, `tests/`). Entries share their text with the other documents of the module, so the same anchor is described the same way everywhere.

### `COMM_db_index_stats` [@ANCHOR: COMM_db_index_stats]

The model `database.index.stat`, the index-health list, backed by the view of `COMM_db_index_stat_init`; global and read-only. Default order `idx_scan asc`, so unused indexes come first.

| Field | Type | Meaning |
|---|---|---|
| `table_name` | Char | Table the index belongs to. |
| `index_name` | Char | Index name. |
| `idx_scan` | Integer | Number of scans that used it since statistics were last reset. |
| `index_size_kb` | Float | Size in kilobytes. |

Unique indexes are excluded because they enforce constraints and cannot simply be dropped. Access: Database Manager group, read only. Tests: `test_04_views` and `test_03_db_index_stats` (which in fact exercises `cron_check_bloat`; to-do: point it at this model).

### `COMM_vacuum_analyze` [@ANCHOR: COMM_vacuum_analyze]

`DatabaseTableStat.action_vacuum_analyze(self)` (`@public_rpc`), the "Vacuum Analyze" button on the table-statistics list. It runs `vacuumdb -z -t <table> ... <dbname>` once for all selected rows that have a `table_name`, in a subprocess so the vacuum is outside the ORM transaction (a vacuum cannot run inside one).

| Input | Detail |
|---|---|
| `self` | One or more `database.table.stat` records; rows without a table name are ignored. |
| environment | Built minimally (ADR 0044): `PATH`, `PGPORT` (default 5432), `PGUSER` (default `odoo`) and `PGHOST` only if set. No password is passed: with `PGHOST` unset `vacuumdb` uses the local socket and peer authentication. |

**Returns:** `True` (also when nothing was selected). **Errors:** `UserError` with the stderr text when `vacuumdb` exits non-zero, "timed out" after 600 seconds, or on `OSError`. **Safety:** `shell=False`, table names go as separate argv items, the binary comes from `COMM_db_table_stat_get_executable`. Blocking for up to ten minutes, so it holds a web worker. Idempotent and safe to repeat. Tests: `test_01_vacuum_analyze`, `test_01a_vacuum_uses_the_local_socket_and_never_a_password`, `test_01b_vacuum_analyze_failures`.

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

### `COMM_pg_optimize_wizard` [@ANCHOR: COMM_pg_optimize_wizard]

`PgOptimizeWizard.action_apply_optimizations(self)` (`@public_rpc`) on the transient model `pg.optimize.wizard`, which computes PostgreSQL tuning values from the hardware and writes them with `ALTER SYSTEM`.

| Field | Type | Meaning |
|---|---|---|
| `ram_gb` | Integer | Server RAM in GB, default 8, must be above 0. |
| `cpu_cores` | Integer | Cores, default 4, above 0. |
| `storage_type` | Selection | `ssd` (default) or `hdd`. |
| `max_connections` | Integer | Default 200, above 0. |

**Formulas:** `shared_buffers` 25% of RAM; `effective_cache_size` 75%; `maintenance_work_mem` 5% capped at 1024 MB; `work_mem` = 25% of RAM / `max_connections`, at least 4 MB; `random_page_cost` 1.1 (ssd) or 4.0; `max_worker_processes` and `max_parallel_workers` = cores; `max_parallel_workers_per_gather` = max(2, cores/2). **Behaviour:** values go through `psycopg2.sql` (`Identifier` and `Literal`) on a separate registry cursor, followed by `pg_reload_conf()`. **Returns** a sticky warning notification that `shared_buffers` and `max_connections` need a PostgreSQL restart. **Errors:** `UserError` for a non-positive input. Writes `postgresql.auto.conf`, a persistent server-wide change; the database user must be a superuser. Tests: `test_01_optimization_wizard`, `test_01b_optimization_wizard_errors`.

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

### `COMM_db_terminate_backend` [@ANCHOR: COMM_db_terminate_backend]

`DatabaseActivity.action_terminate_backend(self)` (`@public_rpc`), the "Terminate Session" button. It collects the `pid` of every selected row, switches to the `user_database_management_service` environment and runs one statement, `SELECT pg_terminate_backend(pid) FROM unnest(%s) AS pid`, with the pid list as a bound array (one round trip for any number of sessions). **Returns** `True`, also when the list is empty. Errors from the database propagate. The service account is a PostgreSQL role with enough privilege to signal the target backends; a pid that no longer exists or belongs to a superuser is simply not terminated (`pg_terminate_backend` returns false and the result is not inspected). A terminated client loses its transaction. Not undoable. Tests: `test_03_terminate_backend`.

### `COMM_db_slow_queries` [@ANCHOR: COMM_db_slow_queries]

The model `database.query.stat`, the slow-query list (APM). Read-only, backed by the view of `COMM_db_query_stat_init` over `pg_stat_statements` for the current database; global across companies. Default order `total_time desc`.

| Field | Type | Meaning |
|---|---|---|
| `query` | Text | Normalised SQL text. |
| `calls` | Integer | Execution count. |
| `total_time` | Float | Total execution time, milliseconds (`total_exec_time`). |
| `mean_time` | Float | Mean execution time, milliseconds. |

Without the extension the view holds one placeholder row whose `query` explains the missing setup. **Buttons:** `action_install_extension` (`COMM_db_install_extension`), `action_reset_stats` (`COMM_db_reset_stats`) and `action_explain_query` (`COMM_db_explain_query`). Access: Database Manager group, read only. Tests: `test_04_views`, `test_06_query_stats_ops`.

### `COMM_test_dba_cron` [@ANCHOR: COMM_test_dba_cron]

Test id for the scheduled bloat check and the stat models. `test_02_bloat_cron` patches `_get_service_env` with a mock and triggers the cron record `database_management.cron_check_bloat` (job "DBA: Check Autovacuum Bloat", model `database.table.stat`, run as `user_database_management_service`, daily, code `model.cron_check_bloat()`, with the manager group named on the job so Odoo allows the action). The same anchor is cited by `data/cron.xml` as the audit-ignore justification and on the replication and index-advisor models. Failure means the cron record or `cron_check_bloat` no longer runs. See `COMM_bloat_alert_synergy`.

### `COMM_bloat_alert_synergy` [@ANCHOR: COMM_bloat_alert_synergy]

`DatabaseTableStat.cron_check_bloat(self)` (`@api.private`, `@api.model`), run once a day by the cron "DBA: Check Autovacuum Bloat" as the service account. It searches `database.table.stat` for tables with `dead_percent > 20` and `dead_tuples > 10000` (at most 1000 rows). If any exist it opens one incident through `pager.incident.report_incident` (the project's own `pager_duty` module) with source "DBA Autovacuum Monitor", severity `medium` and a description listing each table with its dead percentage and asking for a manual Vacuum Analyze. **Returns** nothing. **Errors:** `AccessError` and `UserError` from the pager service are logged as warnings and swallowed, so a misconfigured pager never fails the cron. Elevation uses `_get_service_env("pager_duty.user_pager_service_internal")`. Repeated runs open a new incident each day while the bloat persists. Tests: `test_02b_bloat_alert_synergy` and `COMM_test_dba_cron`.
