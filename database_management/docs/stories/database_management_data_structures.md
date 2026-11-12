# Data structures

### `DatabaseTableStat` [@ANCHOR: database_management:database_management_db_stats_database_table_stat]

**Purpose and location.** Model description: Database Table Statistics (Bloat & Vacuum). Defined in `database_management/models/db_stats.py:18` as an Odoo model; bases: `models.Model`. (model `database.table.stat`; default order "dead_percent desc".) It is a stored model with its own database table.

| Field | Type | Meaning |
|---|---|---|
| `name` | `Char` | Name (default=`lambda self: self._description`) |
| `table_name` | `Char` | Table Name (readonly=`True`) |
| `live_tuples` | `Integer` | Live Tuples (readonly=`True`) |
| `dead_tuples` | `Integer` | Dead Tuples (readonly=`True`) |
| `dead_percent` | `Float` | Dead % (Bloat) (readonly=`True`) |
| `total_size_mb` | `Float` | Total Size (MB) (readonly=`True`) |
| `cache_hit_percent` | `Float` | Cache Hit %: Percentage of data reads satisfied by RAM rather than Disk I/O. (readonly=`True`) |

**Class attributes.** `_name = "database.table.stat"`; `_description = "Database Table Statistics (Bloat & Vacuum)"`; `_auto = False`; `_order = "dead_percent desc"`.

**Invariants, ownership and concurrency.** Odoo runs each request in one database transaction; rows are created, read and changed through the ORM with the caller's access rights, and nothing here is shared between threads except through the database. Constraints and defaults are those in the field list above; no other invariant is stated in the class docstring.

**Methods.** `init`, `_get_executable`, `action_vacuum_analyze`, `cron_check_bloat`.

**See also:** test `test_01_vacuum_analyze (tests/test_db_management.py)`.

### `DatabaseQueryStat` [@ANCHOR: database_management:database_management_db_stats_database_query_stat]

**Purpose and location.** Model description: Slow Query Tracking. Defined in `database_management/models/db_stats.py:153` as an Odoo model; bases: `models.Model`. (model `database.query.stat`; default order "total_time desc".) It is a stored model with its own database table.

| Field | Type | Meaning |
|---|---|---|
| `name` | `Char` | Name (default=`lambda self: self._description`) |
| `query` | `Text` | SQL Query (readonly=`True`) |
| `calls` | `Integer` | Execution Count (readonly=`True`) |
| `total_time` | `Float` | Total Time (ms) (readonly=`True`) |
| `mean_time` | `Float` | Mean Time (ms) (readonly=`True`) |

**Class attributes.** `_name = "database.query.stat"`; `_description = "Slow Query Tracking"`; `_auto = False`; `_order = "total_time desc"`.

**Invariants, ownership and concurrency.** Odoo runs each request in one database transaction; rows are created, read and changed through the ORM with the caller's access rights, and nothing here is shared between threads except through the database. Constraints and defaults are those in the field list above; no other invariant is stated in the class docstring.

**Methods.** `init`, `action_install_extension`, `action_reset_stats`, `action_explain_query`.

**See also:** test `test_04_views (tests/test_db_management.py)`.

### `DatabaseActivity` [@ANCHOR: database_management:database_management_db_stats_database_activity]

**Purpose and location.** Model description: Active Database Sessions. Defined in `database_management/models/db_stats.py:327` as an Odoo model; bases: `models.Model`. (model `database.activity`; default order "duration desc".) It is a stored model with its own database table.

| Field | Type | Meaning |
|---|---|---|
| `name` | `Char` | Name (default=`lambda self: self._description`) |
| `pid` | `Integer` | PID (readonly=`True`) |
| `usename` | `Char` | User (readonly=`True`) |
| `state` | `Char` | State (readonly=`True`) |
| `query` | `Text` | Active Query (readonly=`True`) |
| `duration` | `Float` | Duration (s) (readonly=`True`) |

**Class attributes.** `_name = "database.activity"`; `_description = "Active Database Sessions"`; `_auto = False`; `_order = "duration desc"`.

**Invariants, ownership and concurrency.** Odoo runs each request in one database transaction; rows are created, read and changed through the ORM with the caller's access rights, and nothing here is shared between threads except through the database. Constraints and defaults are those in the field list above; no other invariant is stated in the class docstring.

**Methods.** `init`, `action_terminate_backend`.

**See also:** test `test_03_terminate_backend (tests/test_db_management.py)`.

### `DatabaseIndexStat` [@ANCHOR: database_management:database_management_db_stats_database_index_stat]

**Purpose and location.** Model description: Database Index Health. Defined in `database_management/models/db_stats.py:382` as an Odoo model; bases: `models.Model`. (model `database.index.stat`; default order "idx_scan asc".) It is a stored model with its own database table.

| Field | Type | Meaning |
|---|---|---|
| `name` | `Char` | Name (default=`lambda self: self._description`) |
| `table_name` | `Char` | Table Name (readonly=`True`) |
| `index_name` | `Char` | Index Name (readonly=`True`) |
| `idx_scan` | `Integer` | Total Scans (Usage) (readonly=`True`) |
| `index_size_kb` | `Float` | Size (KB) (readonly=`True`) |

**Class attributes.** `_name = "database.index.stat"`; `_description = "Database Index Health"`; `_auto = False`; `_order = "idx_scan asc"`.

**Invariants, ownership and concurrency.** Odoo runs each request in one database transaction; rows are created, read and changed through the ORM with the caller's access rights, and nothing here is shared between threads except through the database. Constraints and defaults are those in the field list above; no other invariant is stated in the class docstring.

**Methods.** `init`.

**See also:** test `test_01_vacuum_analyze (tests/test_db_management.py)`.

### `DatabaseReplicationStat` [@ANCHOR: database_management:database_management_db_stats_database_replication_stat]

**Purpose and location.** Model description: Database Replication Statistics. Defined in `database_management/models/db_stats.py:418` as an Odoo model; bases: `models.Model`. (model `database.replication.stat`.) It is a stored model with its own database table.

| Field | Type | Meaning |
|---|---|---|
| `name` | `Char` | Name (default=`lambda self: self._description`) |
| `usename` | `Char` | User (readonly=`True`) |
| `application_name` | `Char` | Application (readonly=`True`) |
| `client_addr` | `Char` | Client IP (readonly=`True`) |
| `state` | `Char` | State (readonly=`True`) |
| `sent_lsn` | `Char` | Sent LSN (readonly=`True`) |
| `write_lsn` | `Char` | Write LSN (readonly=`True`) |
| `flush_lsn` | `Char` | Flush LSN (readonly=`True`) |
| `replay_lsn` | `Char` | Replay LSN (readonly=`True`) |
| `write_lag` | `Char` | Write Lag (readonly=`True`) |
| `flush_lag` | `Char` | Flush Lag (readonly=`True`) |
| `replay_lag` | `Char` | Replay Lag (readonly=`True`) |
| `sync_priority` | `Integer` | Priority (readonly=`True`) |
| `sync_state` | `Char` | Sync State (readonly=`True`) |

**Class attributes.** `_name = "database.replication.stat"`; `_description = "Database Replication Statistics"`; `_auto = False`.

**Invariants, ownership and concurrency.** Odoo runs each request in one database transaction; rows are created, read and changed through the ORM with the caller's access rights, and nothing here is shared between threads except through the database. Constraints and defaults are those in the field list above; no other invariant is stated in the class docstring.

**Methods.** `init`.

**See also:** test `test_04_views (tests/test_db_management.py)`.

### `DatabaseIndexAdvisor` [@ANCHOR: database_management:database_management_db_stats_database_index_advisor]

**Purpose and location.** Model description: Index Recommendation Advisor. Defined in `database_management/models/db_stats.py:469` as an Odoo model; bases: `models.Model`. (model `database.index.advisor`; default order "seq_scan desc".) It is a stored model with its own database table.

| Field | Type | Meaning |
|---|---|---|
| `name` | `Char` | Name (default=`lambda self: self._description`) |
| `table_name` | `Char` | Table Name (readonly=`True`) |
| `seq_scan` | `Integer` | Sequential Scans (readonly=`True`) |
| `seq_tup_read` | `Integer` | Tuples Read (Seq) (readonly=`True`) |
| `idx_scan` | `Integer` | Index Scans (readonly=`True`) |
| `table_size_mb` | `Float` | Table Size (MB) (readonly=`True`) |

**Class attributes.** `_name = "database.index.advisor"`; `_description = "Index Recommendation Advisor"`; `_auto = False`; `_order = "seq_scan desc"`.

**Invariants, ownership and concurrency.** Odoo runs each request in one database transaction; rows are created, read and changed through the ORM with the caller's access rights, and nothing here is shared between threads except through the database. Constraints and defaults are those in the field list above; no other invariant is stated in the class docstring.

**Methods.** `init`.

**See also:** test `test_01_vacuum_analyze (tests/test_db_management.py)`.

### `PgExplainWizard` [@ANCHOR: database_management:database_management_db_stats_pg_explain_wizard]

**Purpose and location.** Model description: PostgreSQL Explain Wizard. Defined in `database_management/models/db_stats.py:506` as an Odoo model; bases: `models.TransientModel`. (model `pg.explain.wizard`.) It is a transient (wizard) model: its rows are short-lived and removed by Odoo's vacuum.

| Field | Type | Meaning |
|---|---|---|
| `name` | `Char` | Name (default=`lambda self: self._description`) |
| `query` | `Text` | SQL Query (readonly=`True`) |
| `explain_plan` | `Text` | Explain Plan (readonly=`True`) |

**Class attributes.** `_name = "pg.explain.wizard"`; `_description = "PostgreSQL Explain Wizard"`.

**Invariants, ownership and concurrency.** Odoo runs each request in one database transaction; rows are created, read and changed through the ORM with the caller's access rights, and nothing here is shared between threads except through the database. Constraints and defaults are those in the field list above; no other invariant is stated in the class docstring.

**Methods.** `action_close`.

**See also:** test `test_04_views (tests/test_db_management.py)`.

### `DatabasePgSetting` [@ANCHOR: database_management:database_management_pg_config_database_pg_setting]

**Purpose and location.** Model description: PostgreSQL Configuration Parameter. Defined in `database_management/models/pg_config.py:15` as an Odoo model; bases: `models.Model`. (model `database.pg.setting`; default order "category asc, name asc".) It is a stored model with its own database table.

| Field | Type | Meaning |
|---|---|---|
| `name` | `Char` | Parameter (readonly=`True`) |
| `setting` | `Char` | Current Value (readonly=`True`) |
| `unit` | `Char` | Unit (readonly=`True`) |
| `category` | `Char` | Category (readonly=`True`) |
| `short_desc` | `Char` | Description (readonly=`True`) |
| `context` | `Char` | Context (readonly=`True`) |
| `pending_restart` | `Boolean` | Pending Restart (readonly=`True`) |

**Class attributes.** `_name = "database.pg.setting"`; `_description = "PostgreSQL Configuration Parameter"`; `_auto = False`; `_order = "category asc, name asc"`.

**Invariants, ownership and concurrency.** Odoo runs each request in one database transaction; rows are created, read and changed through the ORM with the caller's access rights, and nothing here is shared between threads except through the database. Constraints and defaults are those in the field list above; no other invariant is stated in the class docstring.

**Methods.** `init`.

**See also:** test `test_02_view_models_readonly (tests/test_db_security.py)`.

### `PgOptimizeWizard` [@ANCHOR: database_management:database_management_pg_config_pg_optimize_wizard]

**Purpose and location.** Model description: PostgreSQL Optimization Wizard. Defined in `database_management/models/pg_config.py:55` as an Odoo model; bases: `models.TransientModel`. (model `pg.optimize.wizard`.) It is a transient (wizard) model: its rows are short-lived and removed by Odoo's vacuum.

| Field | Type | Meaning |
|---|---|---|
| `name` | `Char` | Name (default=`lambda self: self._description`) |
| `ram_gb` | `Integer` | Total Server RAM (GB) (required=`True`; default=`8`) |
| `cpu_cores` | `Integer` | Total CPU Cores (required=`True`; default=`4`) |
| `storage_type` | `Selection` | no label or help in the source (values `ssd`, `hdd`; required=`True`; default=`"ssd"`) |
| `max_connections` | `Integer` | Max Connections (required=`True`; default=`200`) |

**Class attributes.** `_name = "pg.optimize.wizard"`; `_description = "PostgreSQL Optimization Wizard"`.

**Invariants, ownership and concurrency.** Odoo runs each request in one database transaction; rows are created, read and changed through the ORM with the caller's access rights, and nothing here is shared between threads except through the database. Constraints and defaults are those in the field list above; no other invariant is stated in the class docstring.

**Methods.** `action_apply_optimizations`.

**See also:** test `test_01_security (tests/test_db_security.py)`.

### `PgHaWizard` [@ANCHOR: database_management:database_management_pg_config_pg_ha_wizard]

**Purpose and location.** Model description: High Availability Failover Wizard. Defined in `database_management/models/pg_config.py:125` as an Odoo model; bases: `models.TransientModel`. (model `pg.ha.wizard`.) It is a transient (wizard) model: its rows are short-lived and removed by Odoo's vacuum.

| Field | Type | Meaning |
|---|---|---|
| `name` | `Char` | Name (default=`lambda self: self._description`) |
| `cluster_name` | `Char` | Cluster Name (required=`True`; default=`"hams_cluster"`) |
| `primary_ip` | `Char` | Primary Node IP (required=`True`; default=`"10.0.0.1"`) |
| `secondary_ip` | `Char` | Secondary Node IP (required=`True`; default=`"10.0.0.2"`) |
| `etcd_hosts` | `Char` | Etcd Hosts: Comma-separated list of etcd hosts (e.g., 10.0.0.1:2379,10.0.0.2:2379) (required=`True`; default=`"etcd:2379"`) |
| `replication_user` | `Char` | Replication User (required=`True`; default=`"replicator"`) |
| `replication_pass` | `Char` | Replication Password (required=`True`; default=`lambda self: secrets.token_urlsafe(24)`) |
| `superuser_user` | `Char` | Superuser Name (required=`True`; default=`"postgres"`) |
| `state` | `Selection` | no label or help in the source (values `input`, `generated`; default=`"input"`) |
| `patroni_primary` | `Text` | Primary Patroni YAML (readonly=`True`) |
| `patroni_secondary` | `Text` | Secondary Patroni YAML (readonly=`True`) |
| `pgbouncer_ini` | `Text` | PgBouncer INI (readonly=`True`) |

**Class attributes.** `_name = "pg.ha.wizard"`; `_description = "High Availability Failover Wizard"`.

**Invariants, ownership and concurrency.** Odoo runs each request in one database transaction; rows are created, read and changed through the ORM with the caller's access rights, and nothing here is shared between threads except through the database. Constraints and defaults are those in the field list above; no other invariant is stated in the class docstring.

**Methods.** `_get_executable`, `_validate_inputs`, `action_generate`.

**See also:** test `test_01_security (tests/test_db_security.py)`.
