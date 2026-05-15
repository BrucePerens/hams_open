# Database Management (`database_management`)

*Copyright © Bruce Perens K6BP. Licensed under the GNU Affero General Public License v3.0 (AGPL-3.0).*

Provides an enterprise-grade Application Performance Monitoring (APM) and DBA suite directly within Odoo. This module is designed for the Open Source community and supports both Odoo Community (with `manual_library`) and Odoo Enterprise (with `knowledge`).

---

# Technical Documentation

## Architecture
The `database_management` module provides a comprehensive DBA toolkit integrated directly into the Odoo interface. It follows a Zero-Sudo architecture, utilizing micro-privilege service accounts for sensitive operations.

### Key Components:
*   **Stat Tracking:** Connects to native PostgreSQL stat views (`pg_stat_user_tables`, `pg_stat_activity`, `pg_statio_user_tables`, `pg_stat_statements`) to track bloat, cache hits, and slow queries `[@ANCHOR: db_index_stats]`.
*   **Active Orchestration:** Exposes `pg_terminate_backend` `[@ANCHOR: db_terminate_backend]` and `VACUUM ANALYZE` `[@ANCHOR: vacuum_analyze]` commands to the GUI for immediate incident remediation.
*   **Configuration & Tuning:** Evaluates `pg_settings` and provides wizards `[@ANCHOR: pg_optimize_wizard]` to dynamically write to `postgresql.auto.conf` via parameterized `ALTER SYSTEM` commands.
*   **HA Generation:** Orchestrates exact configuration templates `[@ANCHOR: pg_ha_wizard]` for Patroni and PgBouncer to facilitate horizontal scaling.
*   **Self-Healing Dependencies:** Automatically downloads the `etcd` binary from GitHub if missing when generating HA configurations. Detects and utilizes the OS `vacuumdb` binary via `subprocess` for autovacuum overrides, preventing transaction block errors in the ORM.
*   **Dynamic Documentation:** Automatically installs documentation into `knowledge.article` if either `manual_library` or `knowledge` is present. This is handled via the `knowledge_docs` manifest facility and `zero_sudo` bootstrap.

## Security
*   **Micro-Privileges:** All DBA operations must use the `user_database_management_service` service account when elevating privileges.
*   **Zero-Sudo:** The module strictly avoids `.sudo()` and instead uses `with_user(svc_uid)` or the service account cursor for secure privilege escalation.
*   **Strict Access:** Access is strictly hard-locked to the `base.group_system` (System Administrator) role.
*   **SQLi Defense:** The module strictly utilizes the `psycopg2.sql` module to safely format table names and parameter values before executing raw queries against the cursor.
*   **Binary Execution:** Uses `zero_sudo.security.utils` to ensure only authorized binaries (`vacuumdb`, `etcd`, `patroni`, `pgbouncer`) are executed.

## Automated Subsystems & Alerts
* **Bloat Alerts:** Integrates with Pager Duty to automatically alert SREs on excessive table/index bloat anomalies `[@ANCHOR: bloat_alert_synergy]`.

## Architectural Stories & Journeys

### User Stories
*   [Monitor Table and Index Health](database_management/docs/stories/user_stories.md) `[@ANCHOR: db_index_stats]`
*   [Track Slow Queries](database_management/docs/stories/user_stories.md) `[@ANCHOR: db_slow_queries]`
*   [Manually Reclaim Disk Space](database_management/docs/stories/user_stories.md) `[@ANCHOR: vacuum_analyze]`
*   [Terminate Runaway Sessions](database_management/docs/stories/user_stories.md) `[@ANCHOR: db_active_sessions]`
*   [Tune PostgreSQL Parameters](database_management/docs/stories/user_stories.md) `[@ANCHOR: pg_optimize_wizard]`
*   [Configure High Availability](database_management/docs/stories/user_stories.md) `[@ANCHOR: pg_ha_wizard]`
*   [Automated Bloat Alerts](database_management/docs/stories/user_stories.md) `[@ANCHOR: bloat_alert_synergy]`
*   [Seamless Documentation Access](database_management/docs/stories/user_stories.md) `[@ANCHOR: db_doc_injection]`

### User Journeys
1.  [Identifying and Resolving Table Bloat](database_management/docs/journeys/user_journeys.md)
2.  [Optimizing Database for New Hardware](database_management/docs/journeys/user_journeys.md)
3.  [Scaling with High Availability](database_management/docs/journeys/user_journeys.md)
4.  [Terminating Runaway Sessions](database_management/docs/journeys/user_journeys.md)
5.  [Investigating Slow Queries](database_management/docs/journeys/user_journeys.md)
6.  [Proactive Incident Management](database_management/docs/journeys/user_journeys.md)

## Testing & Verification
* **Cron Execution:** Database stat collection via cron is verified by `[@ANCHOR: test_dba_cron]`.
* **View Rendering:** Rendering of configuration and DBA dashboards is verified by `[@ANCHOR: test_dba_view]` and `[@ANCHOR: test_pg_config_views]`.
