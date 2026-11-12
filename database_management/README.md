# Database Management (`database_management`)

*Copyright © Bruce Perens K6BP. Licensed under the GNU Affero General Public License v3.0 or later (AGPL-3.0-or-later).*

The `database_management` module provides a comprehensive suite of Database Administration (DBA) and Application Performance Monitoring (APM) tools directly within the Odoo interface. It is designed to empower Site Reliability Engineers (SREs) and administrators with the ability to monitor, tune, and scale the PostgreSQL database without requiring shell access.

---

## 🚀 Key Features

*   **Stat Tracking:** Real-time visibility into table bloat, index usage, and cache hit ratios.
*   **Slow Query Monitoring (APM):** Identifies the most resource-intensive SQL queries using `pg_stat_statements`.
*   **Active Session Management:** View and terminate runaway database sessions using batch operations for improved performance.
*   **Slow Query Explain:** Generate `EXPLAIN (ANALYZE, BUFFERS)` plans for slow queries to diagnose performance bottlenecks.
*   **Index Advisor:** Recommends potentially missing indexes based on sequential scan statistics and table size. It lists candidate tables (more than 100 sequential scans and larger than 10 MB); it does not propose specific index definitions.
*   **Replication Monitoring:** Real-time tracking of PostgreSQL replication lag and status across the cluster.
*   **Performance Tuning Wizard:** Automatically calculates optimal PostgreSQL parameters based on hardware specifications and applies them via `ALTER SYSTEM`.
*   **High Availability Orchestrator:** Generates production-ready configurations for a two-node cluster: a Patroni YAML file for each node (pointing at your existing etcd hosts) and a PgBouncer INI file. It displays them for copying; it does not deploy anything, and it does not generate etcd's own configuration.
*   **Automated Alerts:** Integrates with PagerDuty to notify SREs when table bloat exceeds critical thresholds. "PagerDuty" here is this project's own `pager_duty` Odoo module, not the commercial service. A daily cron job opens one incident (severity medium) listing every table with more than 20% dead tuples and more than 10,000 dead tuples.
*   **Zero-Sudo Architecture:** Ensures all operations are performed with minimum necessary privileges using dedicated service accounts. "Zero-Sudo" is this codebase's rule, enforced by the `zero_sudo` module, that Odoo's `.sudo()` is never used.

---

## 🛠 Architecture & Security

### Micro-Privilege Architecture
This module strictly adheres to a Zero-Sudo policy. Session termination (`pg_terminate_backend`), query-statistics reset (`pg_stat_statements_reset`) and Explain run as the `user_database_management_service` service account. Privilege elevation is handled via `_get_service_env()` from the `zero_sudo` module, ensuring that no `sudo()` calls are used in the codebase. The Optimization Wizard's `ALTER SYSTEM` statements do not use the service account; they run on a separate database cursor opened from the Odoo registry. The service account is an Odoo user, not a PostgreSQL role: its SQL still runs over Odoo's own database connection, so PostgreSQL-level permissions are those of Odoo's database user.

### Multi-Tenant & Global Awareness
Models in this module are designed to be **logically global**. Since they monitor PostgreSQL system statistics (such as `pg_stat_user_tables`, `pg_stat_statements`, and `pg_stat_replication`), the data they provide represents the aggregate state of the whole current database (and, for replication, of the whole PostgreSQL server), not of any one Odoo company. In multi-tenant environments where multiple Odoo companies share a single database, these statistics correctly reflect the performance and health of the shared infrastructure.

### Security Hardening
*   **SQL Injection Prevention:** The Optimization Wizard builds its `ALTER SYSTEM` statements with the `psycopg2.sql` library (`sql.Identifier` for the parameter name, `sql.Literal` for the value). Other runtime values are passed as bound query parameters; Explain passes the query text as a parameter to the `dba_explain_query()` database function and accepts only `SELECT` and `WITH` queries. `COMM_pg_optimize_wizard`

*   **Input Validation:** The HA wizard parses node IP addresses with Python's `ipaddress` module, requires cluster and user names to match `^[a-zA-Z0-9_]+$`, requires etcd hosts as comma-separated `host:port` pairs, and requires a replication password of at least 8 characters with no characters that would break the generated YAML. `COMM_pg_ha_wizard`

*   **Binary Safety:** Execution of external binaries (e.g., `vacuumdb`) is managed via `zero_sudo.security.utils._ensure_executable()`, which uses the binary found on `PATH` or, if there is none, asks `binary_downloader` to install it. `vacuumdb` runs without a shell and with a minimal environment (`PATH`, `PGPORT`, `PGUSER`, and `PGHOST` if set; no password: by default it uses the local socket with peer authentication, as the Odoo process does). `COMM_vacuum_analyze`
*   **Access Control:** Model access rights are granted to the `database_management.group_database_management_manager` group ("Database Manager", listed under the "Database Management" privilege). Odoo's `base.group_system` (Settings) implies that group, and the **Database & SRE** menu is shown only to `base.group_system`.

### Components
*   **Stat Views:** Native PostgreSQL statistics are exposed via Odoo models (`database.table.stat`, `database.index.stat`, `database.query.stat`, `database.activity`, `database.replication.stat`, `database.index.advisor`, `database.pg.setting`) using PostgreSQL views. `COMM_db_index_stats`

*   **Vacuum Automation:** Manual `VACUUM ANALYZE` is triggered via `subprocess` calling `vacuumdb`, bypassing Odoo's transaction blocks to allow physical cleanup. `COMM_vacuum_analyze`

*   **Configuration Management:** The Optimization Wizard `COMM_pg_optimize_wizard` writes to `postgresql.auto.conf` and reloads the configuration.

---

## 📦 External Dependencies

This module requires the following external binaries or Python dependencies. Only `vacuumdb` is declared in `__manifest__.py`; the other three are needed only by the High Availability Orchestrator, which checks for them on the Odoo host before generating configuration:
*   `vacuumdb`: PostgreSQL client application for cleaning databases.
*   `patroni`: High availability solution for PostgreSQL.
*   `pgbouncer`: Lightweight connection pooler for PostgreSQL.
*   `etcd`: Distributed key-value store for Patroni.

---

## 📚 Documentation & Help

User-facing documentation is available directly within the Knowledge module, when it is installed: `zero_sudo`'s knowledge-doc bootstrap creates the article from this module's `knowledge_docs` manifest entry.
*   **Guide:** `Database Management Guide` (installed from `data/documentation.html`).

---

## 🧪 Testing & Verification

The module includes an exhaustive test suite covering standard and integration scenarios:
*   **Standard Tests:** Verify model logic, view rendering, and security constraints. `COMM_test_dba_view`

*   **Integration Tests:** Simulate `vacuumdb` execution and HA configuration generation. `COMM_test_dba_cron`

*   **Security Tests:** Verify that only authorized users can access sensitive DBA tools and that standard users are isolated. `COMM_test_db_security`

*   **UI Tours:** Automated browser tours verify the end-to-end user journeys for bloat management and slow query analysis. `COMM_test_db_bloat_tour`

---

## 🔄 Semantic Anchors (Internal Reference)

*   `COMM_db_index_stats`: Stats collection for tables and indexes.

*   `COMM_db_terminate_backend`: Logic for killing active sessions.

*   `COMM_vacuum_analyze`: Subprocess orchestration for `vacuumdb`.

*   `COMM_pg_optimize_wizard`: Hardware-based tuning calculations.

*   `COMM_pg_ha_wizard`: HA cluster configuration generation.

*   `COMM_db_slow_queries`: APM tracking via `pg_stat_statements`.

*   `COMM_db_replication_stats`: Replication lag monitoring.

*   `COMM_bloat_alert_synergy`: PagerDuty integration logic.

*   `COMM_db_doc_injection`: Documentation bootstrap verification.

## Anchor reference

One entry per anchor named in this document, read from the module code (`models/db_stats.py`, `models/pg_config.py`, `data/`, `views/`, `tests/`). Entries share their text with the other documents of the module, so the same anchor is described the same way everywhere.

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

### `COMM_vacuum_analyze` [@ANCHOR: COMM_vacuum_analyze]

`DatabaseTableStat.action_vacuum_analyze(self)` (`@public_rpc`), the "Vacuum Analyze" button on the table-statistics list. It runs `vacuumdb -z -t <table> ... <dbname>` once for all selected rows that have a `table_name`, in a subprocess so the vacuum is outside the ORM transaction (a vacuum cannot run inside one).

| Input | Detail |
|---|---|
| `self` | One or more `database.table.stat` records; rows without a table name are ignored. |
| environment | Built minimally (ADR 0044): `PATH`, `PGPORT` (default 5432), `PGUSER` (default `odoo`) and `PGHOST` only if set. No password is passed: with `PGHOST` unset `vacuumdb` uses the local socket and peer authentication. |

**Returns:** `True` (also when nothing was selected). **Errors:** `UserError` with the stderr text when `vacuumdb` exits non-zero, "timed out" after 600 seconds, or on `OSError`. **Safety:** `shell=False`, table names go as separate argv items, the binary comes from `COMM_db_table_stat_get_executable`. Blocking for up to ten minutes, so it holds a web worker. Idempotent and safe to repeat. Tests: `test_01_vacuum_analyze`, `test_01a_vacuum_uses_the_local_socket_and_never_a_password`, `test_01b_vacuum_analyze_failures`.

### `COMM_db_index_stats` [@ANCHOR: COMM_db_index_stats]

The model `database.index.stat`, the index-health list, backed by the view of `COMM_db_index_stat_init`; global and read-only. Default order `idx_scan asc`, so unused indexes come first.

| Field | Type | Meaning |
|---|---|---|
| `table_name` | Char | Table the index belongs to. |
| `index_name` | Char | Index name. |
| `idx_scan` | Integer | Number of scans that used it since statistics were last reset. |
| `index_size_kb` | Float | Size in kilobytes. |

Unique indexes are excluded because they enforce constraints and cannot simply be dropped. Access: Database Manager group, read only. Tests: `test_04_views` and `test_03_db_index_stats` (which in fact exercises `cron_check_bloat`; to-do: point it at this model).

### `COMM_test_dba_view` [@ANCHOR: COMM_test_dba_view]

Test id for the statistics views. `test_04_views` in `tests/test_db_management.py` calls `get_view(view_type="list")` on `database.table.stat`, `database.query.stat`, `database.activity`, `database.index.stat`, `database.replication.stat` and `database.index.advisor`, and `get_view(view_type="form")` on `pg.explain.wizard`, asserting that each compiled arch contains a known field. That proves the views in `views/db_stats_views.xml` compile against the models; they cite this anchor as their audit-ignore reason. It does not read rows. The model `database.table.stat` (`COMM_db_security_prefetch`) cites it too. Failure means a view or a field name changed.

### `COMM_test_dba_cron` [@ANCHOR: COMM_test_dba_cron]

Test id for the scheduled bloat check and the stat models. `test_02_bloat_cron` patches `_get_service_env` with a mock and triggers the cron record `database_management.cron_check_bloat` (job "DBA: Check Autovacuum Bloat", model `database.table.stat`, run as `user_database_management_service`, daily, code `model.cron_check_bloat()`, with the manager group named on the job so Odoo allows the action). The same anchor is cited by `data/cron.xml` as the audit-ignore justification and on the replication and index-advisor models. Failure means the cron record or `cron_check_bloat` no longer runs. See `COMM_bloat_alert_synergy`.

### `COMM_test_db_security` [@ANCHOR: COMM_test_db_security]

Test id for access control. `tests/test_db_security.py` creates standard, website, ham, SWL and public personas and asserts that each gets `AccessError` reading `database.table.stat` and `database.pg.setting` or creating `pg.optimize.wizard` and `pg.ha.wizard` records (`test_01_security`); that every access row of the seven view-backed models has write, create and unlink switched off (`test_02_view_models_readonly`); and that the administrator can read the views and create the wizards (`test_03_admin_success`). This id has no separate tag in the code; the test method cites `COMM_db_security_prefetch`. Unknown whether a test method should carry this id directly (to-do).

### `COMM_test_db_bloat_tour` [@ANCHOR: COMM_test_db_bloat_tour]

Test id for the browser tour `db_management_bloat_tour` (`static/src/tours/db_bloat_tour.js`), started by `test_01_db_bloat_tour` in `tests/test_tours.py`. The tour opens the Apps menu, picks "Database & SRE", waits for the table-statistics list, selects the first row, clicks "Vacuum Analyze Selected" and waits for it to finish. It needs a browser and a populated `pg_stat_user_tables`, so it is slow. A sibling tour, `db_management_slow_query_tour`, is started by `test_02_db_slow_query_tour` (anchor `COMM_test_db_slow_query_tour`). Failure means the menu, the list or the vacuum button changed.

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

### `COMM_bloat_alert_synergy` [@ANCHOR: COMM_bloat_alert_synergy]

`DatabaseTableStat.cron_check_bloat(self)` (`@api.private`, `@api.model`), run once a day by the cron "DBA: Check Autovacuum Bloat" as the service account. It searches `database.table.stat` for tables with `dead_percent > 20` and `dead_tuples > 10000` (at most 1000 rows). If any exist it opens one incident through `pager.incident.report_incident` (the project's own `pager_duty` module) with source "DBA Autovacuum Monitor", severity `medium` and a description listing each table with its dead percentage and asking for a manual Vacuum Analyze. **Returns** nothing. **Errors:** `AccessError` and `UserError` from the pager service are logged as warnings and swallowed, so a misconfigured pager never fails the cron. Elevation uses `_get_service_env("pager_duty.user_pager_service_internal")`. Repeated runs open a new incident each day while the bloat persists. Tests: `test_02b_bloat_alert_synergy` and `COMM_test_dba_cron`.

### `COMM_db_doc_injection` [@ANCHOR: COMM_db_doc_injection]

Documentation bootstrap. `__manifest__.py` lists `knowledge_docs` with one entry, "Database Management Guide" (`data/documentation.html`, icon, category `workspace`). When the Knowledge module is installed, `zero_sudo`'s knowledge-doc bootstrap reads that key and creates or updates the article, so this module has no code of its own for it and uses no elevated privilege. The HTML file itself carries the other anchors of the module as comments, one section per feature (bloat alerts, vacuum, index advisor, index stats, slow queries and explain, active sessions, replication, tuning, high availability). Test: `test_08_doc_injection`, which only checks that the file opens (to-do: assert the article exists once Knowledge is present).
