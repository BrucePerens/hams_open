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
*   **SQL Injection Prevention:** The Optimization Wizard builds its `ALTER SYSTEM` statements with the `psycopg2.sql` library (`sql.Identifier` for the parameter name, `sql.Literal` for the value). Other runtime values are passed as bound query parameters; Explain passes the query text as a parameter to the `dba_explain_query()` database function and accepts only `SELECT` and `WITH` queries. `[@ANCHOR: COMM_pg_optimize_wizard]`

*   **Input Validation:** The HA wizard parses node IP addresses with Python's `ipaddress` module, requires cluster and user names to match `^[a-zA-Z0-9_]+$`, requires etcd hosts as comma-separated `host:port` pairs, and requires a replication password of at least 8 characters with no characters that would break the generated YAML. `[@ANCHOR: COMM_pg_ha_wizard]`

*   **Binary Safety:** Execution of external binaries (e.g., `vacuumdb`) is managed via `zero_sudo.security.utils._ensure_executable()`, which uses the binary found on `PATH` or, if there is none, asks `binary_downloader` to install it. `vacuumdb` runs without a shell and with a minimal environment (`PATH`, `PGHOST`, `PGPORT`, `PGUSER`, and `PGPASSWORD` if set). `[@ANCHOR: COMM_vacuum_analyze]`
*   **Access Control:** Model access rights are granted to the `database_management.group_database_management_manager` group ("Database Manager", listed under the "Database Management" privilege). Odoo's `base.group_system` (Settings) implies that group, and the **Database & SRE** menu is shown only to `base.group_system`.

### Components
*   **Stat Views:** Native PostgreSQL statistics are exposed via Odoo models (`database.table.stat`, `database.index.stat`, `database.query.stat`, `database.activity`, `database.replication.stat`, `database.index.advisor`, `database.pg.setting`) using PostgreSQL views. `[@ANCHOR: COMM_db_index_stats]`

*   **Vacuum Automation:** Manual `VACUUM ANALYZE` is triggered via `subprocess` calling `vacuumdb`, bypassing Odoo's transaction blocks to allow physical cleanup. `[@ANCHOR: COMM_vacuum_analyze]`

*   **Configuration Management:** The Optimization Wizard `[@ANCHOR: COMM_pg_optimize_wizard]` writes to `postgresql.auto.conf` and reloads the configuration.

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
*   **Standard Tests:** Verify model logic, view rendering, and security constraints. `[@ANCHOR: COMM_test_dba_view]`

*   **Integration Tests:** Simulate `vacuumdb` execution and HA configuration generation. `[@ANCHOR: COMM_test_dba_cron]`

*   **Security Tests:** Verify that only authorized users can access sensitive DBA tools and that standard users are isolated. `[@ANCHOR: COMM_test_db_security]`

*   **UI Tours:** Automated browser tours verify the end-to-end user journeys for bloat management and slow query analysis. `[@ANCHOR: COMM_test_db_bloat_tour]`

---

## 🔄 Semantic Anchors (Internal Reference)

*   `[@ANCHOR: COMM_db_index_stats]`: Stats collection for tables and indexes.

*   `[@ANCHOR: COMM_db_terminate_backend]`: Logic for killing active sessions.

*   `[@ANCHOR: COMM_vacuum_analyze]`: Subprocess orchestration for `vacuumdb`.

*   `[@ANCHOR: COMM_pg_optimize_wizard]`: Hardware-based tuning calculations.

*   `[@ANCHOR: COMM_pg_ha_wizard]`: HA cluster configuration generation.

*   `[@ANCHOR: COMM_db_slow_queries]`: APM tracking via `pg_stat_statements`.

*   `[@ANCHOR: COMM_db_replication_stats]`: Replication lag monitoring.

*   `[@ANCHOR: COMM_bloat_alert_synergy]`: PagerDuty integration logic.

*   `[@ANCHOR: COMM_db_doc_injection]`: Documentation bootstrap verification.
