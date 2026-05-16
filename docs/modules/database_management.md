# 🛢 Database Management (`database_management`)

## Architecture
Provides an enterprise-grade Application Performance Monitoring (APM) and DBA suite directly within Odoo. This module is designed for the Open Source community and supports both Odoo Community (with `manual_library`) and Odoo Enterprise (with `knowledge`).
* **Stat Tracking:** Connects to native PostgreSQL stat views (`pg_stat_user_tables`, `pg_stat_activity`, `pg_statio_user_tables`, `pg_stat_statements`) to track bloat, cache hits, and slow queries `[@ANCHOR: db_index_stats]`.
* **Active Orchestration:** Exposes `pg_terminate_backend` `[@ANCHOR: db_terminate_backend]` and `VACUUM ANALYZE` `[@ANCHOR: vacuum_analyze]` commands to the GUI for immediate incident remediation.
* **Configuration & Tuning:** Evaluates `pg_settings` and provides wizards `[@ANCHOR: pg_optimize_wizard]` to dynamically write to `postgresql.auto.conf` via parameterized `ALTER SYSTEM` commands.
* **HA Generation:** Orchestrates exact configuration templates `[@ANCHOR: pg_ha_wizard]` for Patroni and PgBouncer to facilitate horizontal scaling.
* **Self-Healing Dependencies:** Automatically downloads the `etcd` binary from GitHub if missing when generating HA configurations. Detects and utilizes the OS `vacuumdb` binary via `subprocess` for autovacuum overrides, preventing transaction block errors in the ORM.
* **Dynamic Documentation:** Automatically installs documentation into `knowledge.article` if either `manual_library` or `knowledge` is present. This is handled via `_register_hook` to ensure compatibility regardless of installation order.

## Security
* **Strict Access:** Because these tools offer direct, destructive command over the database engine, access is strictly hard-locked to the `base.group_system` (System Administrator) role in the CSV.
* **SQLi Defense:** The module strictly utilizes the `psycopg2.sql` module to safely format table names and parameter values before executing raw queries against the cursor.
* **Least Privilege:** Documentation installation is performed by the `user_database_management_service` account, which is dynamically granted access to `manual_library` if present.

## Automated Subsystems & Alerts
* **Bloat Alerts:** Integrates with Pager Duty to automatically alert SREs on excessive table/index bloat anomalies `[@ANCHOR: bloat_alert_synergy]`.

## Architectural Stories & Journeys

### Stories
* [Database Management Stories](docs/stories/database_management/user_stories.md)

### Journeys
* [Database Management Journeys](docs/journeys/database_management/user_journeys.md)

## Testing & Verification
* **Cron Execution:** Database stat collection via cron is verified by `[@ANCHOR: test_dba_cron]`.
* **View Rendering:** Rendering of configuration and DBA dashboards is verified by `[@ANCHOR: test_dba_view]` and `[@ANCHOR: test_pg_config_views]`.
