# User Journeys: Database Management

## Journey 1: Identifying and Resolving Table Bloat
**Context:** A system administrator notices a slow-down in Odoo and suspects database performance issues.

1. **Dashboard Review:** The administrator opens the "Table Statistics" view `[@ANCHOR: COMM_db_index_stats]` and sorts by "Dead %".
2. **Detection:** They identify the `mail_message` table as having 45% bloat.
3. **Remediation:** They select the table and click the **Vacuum Analyze** button `[@ANCHOR: COMM_vacuum_analyze]`.
4. **Verification:** The system executes the command via `vacuumdb` in a background process to avoid ORM timeouts.
5. **Observation:** The administrator refreshes the view to see the "Dead %" drop and performance improve.

## Journey 2: Optimizing Database for New Hardware
**Context:** The Odoo instance has been migrated to a larger server with 64GB RAM and 16 CPU cores.

1. **Audit:** The DevOps engineer reviews the current settings in the **PostgreSQL Configuration** view `[@ANCHOR: COMM_db_settings_audit]`.

2. **Wizard Activation:** The engineer launches the **PostgreSQL Optimization Wizard** `[@ANCHOR: COMM_pg_optimize_wizard]`.
2. **Configuration:** They enter the new RAM and CPU specifications and select "SSD" storage.
3. **Application:** They click "Apply Optimizations", which executes `ALTER SYSTEM` commands for `shared_buffers`, `work_mem`, and other key parameters.
4. **Reload:** The wizard triggers a configuration reload (`pg_reload_conf()`).
5. **Finalization:** The engineer follows the UI notification to restart the PostgreSQL service to apply the core memory changes.

## Journey 3: Scaling with High Availability
**Context:** To ensure 99.9% uptime, the SRE needs to move from a single DB node to a clustered setup.

1. **HA Setup:** The SRE opens the **HA Failover Wizard** `[@ANCHOR: COMM_pg_ha_wizard]`.
2. **Parameters:** They input the IP addresses for the primary and secondary nodes.
3. **Generation:** They click "Generate Configuration".
4. **Provisioning:** The system checks for the `etcd`, `patroni`, and `pgbouncer` binaries. If `etcd` is missing, it is automatically downloaded.
5. **Deployment:** The SRE copies the generated YAML and INI templates to their respective nodes to initialize the cluster.

## Journey 4: Terminating Runaway Sessions
**Context:** A developer accidentally ran a query without a proper WHERE clause, locking a critical table.

1. **Activity Monitoring:** The DBA opens the "Active Sessions" dashboard `[@ANCHOR: COMM_db_active_sessions]`.
2. **Identification:** They find the PID of the query that has been running for 30 minutes.
3. **Action:** They click the **Terminate Session** button `[@ANCHOR: COMM_db_terminate_backend]`.
4. **Verification:** The system sends a `pg_terminate_backend` signal and the session disappears from the list.

## Journey 5: Investigating Slow Queries
**Context:** Users report that the "Order Entry" screen takes 5 seconds to load.

1. **Slow Query Log:** The System Engineer navigates to the **Slow Query Tracking** view `[@ANCHOR: COMM_db_slow_queries]`.
2. **Identification:** They find an `UPDATE` query that is called 5,000 times an hour with a high mean execution time.
3. **Analysis:** They use the query text to locate the offending Odoo method and implement batch processing.
4. **Verification:** They clear the stats and monitor the view to ensure the query is no longer a bottleneck.

## Journey 6: Proactive Incident Management
**Context:** An unoptimized module is creating many dead tuples during a mass import.

1. **Background Monitoring:** The DBA Autovacuum Monitor cron `[@ANCHOR: COMM_test_dba_cron]` runs every hour.
2. **Threshold Breach:** The cron detects bloat exceeding 20% on several critical tables.
3. **Alerting:** The system automatically triggers a PagerDuty incident `[@ANCHOR: COMM_bloat_alert_synergy]`.
4. **Response:** The on-call SRE receives the alert and navigates to Odoo to perform a manual Vacuum or investigate the source of the bloat.
