# -*- coding: utf-8 -*-
import logging
import os
import subprocess

from psycopg2 import sql as psql
from odoo import models, fields, api, _
from odoo.exceptions import UserError, AccessError
import odoo.tools.sql

_logger = logging.getLogger(__name__)


class DatabaseTableStat(models.Model):
    # [@ANCHOR: db_security_prefetch]
    # This model is logically global as it tracks PostgreSQL internal statistics
    # for the entire database instance, which may be shared by multiple Odoo companies.
    _name = "database.table.stat"
    _description = "Database Table Statistics (Bloat & Vacuum)"
    name = fields.Char(string="Name", default=lambda self: self._description)
    _auto = False
    _order = "dead_percent desc"

    table_name = fields.Char(string="Table Name", readonly=True)
    live_tuples = fields.Integer(string="Live Tuples", readonly=True)
    dead_tuples = fields.Integer(string="Dead Tuples", readonly=True)
    dead_percent = fields.Float(string="Dead % (Bloat)", readonly=True)
    total_size_mb = fields.Float(string="Total Size (MB)", readonly=True)
    cache_hit_percent = fields.Float(
        string="Cache Hit %",
        readonly=True,
        help="Percentage of data reads satisfied by RAM rather than Disk I/O.",
    )

    def init(self):
        odoo.tools.sql.drop_view_if_exists(self.env.cr, self._table)
        self.env.cr.execute(
            """
            CREATE OR REPLACE VIEW database_table_stat AS (
                SELECT
                    row_number() OVER () as id,
                    'SQL View'::varchar as name,
                    t.relname as table_name,
                    t.n_live_tup as live_tuples,
                    t.n_dead_tup as dead_tuples,
                    CASE WHEN t.n_live_tup + t.n_dead_tup > 0 THEN (t.n_dead_tup::float / (t.n_live_tup + t.n_dead_tup)) * 100 ELSE 0 END as dead_percent,
                    pg_total_relation_size(t.relid) / (1024.0 * 1024.0) as total_size_mb,
                    CASE WHEN i.heap_blks_read + i.heap_blks_hit > 0 THEN (i.heap_blks_hit::float / (i.heap_blks_hit + i.heap_blks_read)) * 100 ELSE 0 END as cache_hit_percent
                FROM pg_stat_user_tables t
                JOIN pg_statio_user_tables i ON t.relid = i.relid
            )
        """
        )

    def _get_executable(self, cmd_name):
        return self.env["zero_sudo.security.utils"]._ensure_executable(
            cmd_name,
            svc_xml_id="database_management.user_database_management_service",
            pkg_name="postgresql-client" if cmd_name == "vacuumdb" else cmd_name,
        )

    def action_vacuum_analyze(self):
        # [@ANCHOR: vacuum_analyze]
        # Tests [@ANCHOR: vacuum_analyze]
        exe = self._get_executable("vacuumdb")
        db_name = self.env.cr.dbname
        # ADR-0044: Use minimal environment for subprocess to prevent secret leakage
        env_vars = {
            "PATH": os.environ.get("PATH", ""),
            "PGHOST": os.environ.get("PGHOST", "postgres"),
            "PGPORT": os.environ.get("PGPORT", "5432"),
            "PGUSER": os.environ.get("PGUSER", "odoo"),
        }
        if "PGPASSWORD" in os.environ:
            env_vars["PGPASSWORD"] = os.environ["PGPASSWORD"]

        # Performance: Batching tables to reduce subprocess overhead and connection round-trips
        table_names = self.filtered(lambda r: r.table_name).mapped("table_name")
        if not table_names:
            return True

        cmd = [exe, "-z"]
        for name in table_names:
            cmd.extend(["-t", name])
        cmd.append(db_name)

        try:
            # The subprocess bypasses the active ORM transaction block allowing physical vacuuming
            res = subprocess.run(
                cmd,
                capture_output=True,
                text=True,
                timeout=600,  # Increased timeout for batched execution
                env=env_vars,
                shell=False,
            )
            if res.returncode != 0:
                _logger.warning("Vacuum failed for %s: %s", table_names, res.stderr)
                raise UserError(_("Vacuum failed: %s") % res.stderr)
        except subprocess.TimeoutExpired:
            raise UserError(_("Vacuum timed out for tables: %s") % table_names)
        except OSError as e:
            _logger.exception("Error executing vacuumdb")
            raise UserError(_("Error executing vacuumdb: %s") % str(e))
        return True

    @api.model
    def cron_check_bloat(self):
        # [@ANCHOR: bloat_alert_synergy]
        # Tests [@ANCHOR: bloat_alert_synergy]
        high_bloat = self.env["database.table.stat"].search(
            [("dead_percent", ">", 20.0), ("dead_tuples", ">", 10000)], limit=1000
        )
        if high_bloat:
            # PagerDuty is explicitly declared in __manifest__.py dependencies
            try:
                env_svc = self.env["zero_sudo.security.utils"]._get_service_env(
                    "pager_duty.user_pager_service_internal"
                )
                tables = ", ".join(
                    [f"{t.table_name}" f" ({t.dead_percent:.1f}%)" for t in high_bloat]
                )
                desc = (
                    "Database Bloat Warning! The"
                    " following tables have >20%"
                    " dead tuples and require a"
                    " manual Vacuum Analyze: " + tables
                )
                env_svc["pager.incident"].report_incident(
                    {
                        "source": "DBA Autovacuum Monitor",
                        "severity": "medium",
                        "description": desc,
                    }
                )
            except (AccessError, UserError) as e:
                _logger.warning(
                    "Permission or configuration"
                    " error reporting bloat"
                    " incident: %s",
                    e,
                )


class DatabaseQueryStat(models.Model):
    # [@ANCHOR: db_slow_queries]
    # Tests [@ANCHOR: db_slow_queries]
    # This model is logically global as pg_stat_statements tracks all queries
    # hitting the database, regardless of which Odoo company or website initiated them.
    _name = "database.query.stat"
    name = fields.Char(string="Name", default=lambda self: self._description)
    _description = "Slow Query Tracking"
    _auto = False
    _order = "total_time desc"

    query = fields.Text(string="SQL Query", readonly=True)
    calls = fields.Integer(string="Execution Count", readonly=True)
    total_time = fields.Float(string="Total Time (ms)", readonly=True)
    mean_time = fields.Float(string="Mean Time (ms)", readonly=True)

    def init(self):
        odoo.tools.sql.drop_view_if_exists(self.env.cr, self._table)

        can_query = False
        try:
            with self.env.cr.savepoint():
                # Safely check the postgres catalog and settings to confirm the extension
                # is loaded without triggering a hard 'bad query' ERROR log in odoo.sql_db
                self.env.cr.execute(
                    """
                    SELECT 1 FROM pg_extension e
                    JOIN pg_settings s ON s.name = 'shared_preload_libraries'
                    WHERE e.extname = 'pg_stat_statements'
                    AND s.setting LIKE '%pg_stat_statements%'
                """
                )
                if self.env.cr.fetchone():
                    can_query = True
        except Exception as e:  # audit-ignore-catch-all
            _logger.warning("Graceful degradation check failed: %s", e)

        if can_query:
            self.env.cr.execute(
                """
                CREATE OR REPLACE VIEW database_query_stat AS (
                    SELECT
                        row_number() OVER () as id,
                    'SQL View'::varchar as name,
                        query,
                        calls,
                        total_exec_time as total_time,
                        mean_exec_time as mean_time
                    FROM pg_stat_statements
                    WHERE dbid = (SELECT oid FROM pg_database WHERE datname = current_database())
                )
            """
            )
        else:
            self.env.cr.execute(
                """
                CREATE OR REPLACE VIEW database_query_stat AS (
                    SELECT 1 as id, 'pg_stat_statements not installed or not loaded via shared_preload_libraries in postgresql.conf.' as query, 0 as calls, 0.0 as total_time, 0.0 as mean_time
                )
            """
            )


class DatabaseActivity(models.Model):
    # [@ANCHOR: db_active_sessions]
    # Tests [@ANCHOR: db_active_sessions]
    # This model is logically global as it tracks all active PostgreSQL backends
    # for the current database, representing the aggregate state of the database server.
    _name = "database.activity"
    _description = "Active Database Sessions"
    name = fields.Char(string="Name", default=lambda self: self._description)
    _auto = False
    _order = "duration desc"

    pid = fields.Integer(string="PID", readonly=True)
    usename = fields.Char(string="User", readonly=True)
    state = fields.Char(string="State", readonly=True)
    query = fields.Text(string="Active Query", readonly=True)
    duration = fields.Float(string="Duration (s)", readonly=True)

    def init(self):
        odoo.tools.sql.drop_view_if_exists(self.env.cr, self._table)
        self.env.cr.execute(
            """
            CREATE OR REPLACE VIEW database_activity AS (
                SELECT 'SQL View'::varchar as name,
                    pid as id,
                    pid,
                    usename,
                    state,
                    query,
                    EXTRACT(EPOCH FROM (now() - query_start)) as duration
                FROM pg_stat_activity
                WHERE datname = current_database() AND pid <> pg_backend_pid()
            )
        """
        )

    def action_terminate_backend(self):
        # [@ANCHOR: db_terminate_backend]
        # Tests [@ANCHOR: db_terminate_backend]
        # micro-privilege: Use service account for termination
        utils = self.env["zero_sudo.security.utils"]
        env_svc = utils._get_service_env(
            "database_management.user_database_management_service"
        )

        pids = [rec.pid for rec in self if rec.pid]
        if pids:
            # Performance: Optimize latency by reducing database operation round-trips to only one.
            # We use SELECT unnest() to ensure the termination logic runs entirely on the DB server.
            env_svc.cr.execute(
                "SELECT pg_terminate_backend(pid) FROM unnest(%s) AS pid;",
                (pids,),
            )
        return True


class DatabaseIndexStat(models.Model):
    # [@ANCHOR: db_index_stats]
    # Tests [@ANCHOR: db_index_stats]
    _name = "database.index.stat"
    name = fields.Char(string="Name", default=lambda self: self._description)
    # This model is logically global as it tracks index performance at the PostgreSQL storage layer,
    # which is shared by all Odoo tenants in this database.
    _description = "Database Index Health"
    _auto = False
    _order = "idx_scan asc"

    table_name = fields.Char(string="Table Name", readonly=True)
    index_name = fields.Char(string="Index Name", readonly=True)
    idx_scan = fields.Integer(string="Total Scans (Usage)", readonly=True)
    index_size_kb = fields.Float(string="Size (KB)", readonly=True)

    def init(self):
        odoo.tools.sql.drop_view_if_exists(self.env.cr, self._table)
        self.env.cr.execute(
            """
            CREATE OR REPLACE VIEW database_index_stat AS (
                SELECT
                    row_number() OVER () as id,
                    'SQL View'::varchar as name,
                    relname as table_name,
                    indexrelname as index_name,
                    idx_scan,
                    pg_relation_size(indexrelid) / 1024.0 as index_size_kb
                FROM pg_stat_user_indexes
                JOIN pg_index USING (indexrelid)
                WHERE indisunique IS FALSE
            )
        """
        )


class DatabaseReplicationStat(models.Model):
    # [@ANCHOR: db_replication_stats]
    # This model tracks PostgreSQL replication lag and status for the entire cluster.
    _name = "database.replication.stat"
    _description = "Database Replication Statistics"
    name = fields.Char(string="Name", default=lambda self: self._description)
    _auto = False

    usename = fields.Char(string="User", readonly=True)
    application_name = fields.Char(string="Application", readonly=True)
    client_addr = fields.Char(string="Client IP", readonly=True)
    state = fields.Char(string="State", readonly=True)
    sent_lsn = fields.Char(string="Sent LSN", readonly=True)
    write_lsn = fields.Char(string="Write LSN", readonly=True)
    flush_lsn = fields.Char(string="Flush LSN", readonly=True)
    replay_lsn = fields.Char(string="Replay LSN", readonly=True)
    write_lag = fields.Char(string="Write Lag", readonly=True)
    flush_lag = fields.Char(string="Flush Lag", readonly=True)
    replay_lag = fields.Char(string="Replay Lag", readonly=True)
    sync_priority = fields.Integer(string="Priority", readonly=True)
    sync_state = fields.Char(string="Sync State", readonly=True)

    def init(self):
        odoo.tools.sql.drop_view_if_exists(self.env.cr, self._table)
        self.env.cr.execute(
            """
            CREATE OR REPLACE VIEW database_replication_stat AS (
                SELECT
                    row_number() OVER () as id,
                    'SQL View'::varchar as name,
                    usename,
                    application_name,
                    client_addr,
                    state,
                    sent_lsn::text,
                    write_lsn::text,
                    flush_lsn::text,
                    replay_lsn::text,
                    write_lag::text,
                    flush_lag::text,
                    replay_lag::text,
                    sync_priority,
                    sync_state
                FROM pg_stat_replication
            )
        """
        )


class DatabaseIndexAdvisor(models.Model):
    # [@ANCHOR: db_index_advisor]
    name = fields.Char(string="Name", default=lambda self: self._description)
    # Tests [@ANCHOR: db_index_advisor]
    # This model identifies tables with high sequential scan counts and large
    # sizes that might benefit from additional indexing.
    _name = "database.index.advisor"
    _description = "Index Recommendation Advisor"
    _auto = False
    _order = "seq_scan desc"

    table_name = fields.Char(string="Table Name", readonly=True)
    seq_scan = fields.Integer(string="Sequential Scans", readonly=True)
    seq_tup_read = fields.Integer(string="Tuples Read (Seq)", readonly=True)
    idx_scan = fields.Integer(string="Index Scans", readonly=True)
    table_size_mb = fields.Float(string="Table Size (MB)", readonly=True)

    def init(self):
        odoo.tools.sql.drop_view_if_exists(self.env.cr, self._table)
        self.env.cr.execute(
            """
            CREATE OR REPLACE VIEW database_index_advisor AS (
                SELECT
                    relid as id,
                    relname as table_name,
                    seq_scan,
                    seq_tup_read,
                    idx_scan,
                    pg_total_relation_size(relid) / (1024.0 * 1024.0) as table_size_mb
                FROM pg_stat_user_tables
                WHERE seq_scan > 100 AND pg_total_relation_size(relid) > 10 * 1024 * 1024
            )
        """
        )


class DatabaseQueryStatInherit(models.Model):
    # This consolidation combines methods into a single class block for maintainability
    _inherit = "database.query.stat"

    def action_install_extension(self):
        try:
            self.env.cr.execute("CREATE EXTENSION IF NOT EXISTS pg_stat_statements")
            self.env["database.query.stat"].init()
            return {
                "type": "ir.actions.client",
                "tag": "display_notification",
                "params": {
                    "title": _("Installation Attempted"),
                    "message": _(
                        "Extension created. Please refresh the view to see if queries populate."
                    ),
                    "type": "success",
                    "sticky": False,
                },
            }
        except Exception as e:  # audit-ignore-catch-all
            _logger.warning("Failed to install pg_stat_statements extension: %s", e)
            raise UserError(
                _(
                    "PostgreSQL Extension Installation Failed.\n\n"
                    "Step 1: Ensure your PostgreSQL configuration (postgresql.conf) contains:\n"
                    "shared_preload_libraries = 'pg_stat_statements'\n\n"
                    "Step 2: Restart your PostgreSQL service.\n\n"
                    "Step 3: Ensure your Odoo database user has privileges to run CREATE EXTENSION.\n\n"
                    "Technical Details:\n%s"
                )
                % str(e)
            )

    def action_reset_stats(self):
        # micro-privilege: Use service account for stats reset
        utils = self.env["zero_sudo.security.utils"]
        env_svc = utils._get_service_env(
            "database_management.user_database_management_service"
        )
        try:
            env_svc.cr.execute("SELECT pg_stat_statements_reset()")
            return {
                "type": "ir.actions.client",
                "tag": "display_notification",
                "params": {
                    "title": _("Stats Reset"),
                    "message": _("Query statistics have been cleared."),
                    "type": "success",
                    "sticky": False,
                },
            }
        except Exception as e:  # audit-ignore-catch-all
            _logger.warning("Failed to reset query stats: %s", e)
            raise UserError(_("Could not reset statistics: %s") % str(e))

    def action_explain_query(self):
        # [@ANCHOR: db_explain_query]
        # Tests [@ANCHOR: db_explain_query]
        self.ensure_one()
        utils = self.env["zero_sudo.security.utils"]
        env_svc = utils._get_service_env(
            "database_management.user_database_management_service"
        )
        cr_svc = env_svc.cr

        try:
            query_text = self.query.strip()
            
            query_upper = query_text.upper()
            if not (query_upper.startswith("SELECT") or query_upper.startswith("WITH")):
                msg = _("Only SELECT or WITH queries can be analyzed via Explain.")
                raise UserError(msg)

            cr_svc.execute("SAVEPOINT explain_sp")
            try:
                set_query = psql.SQL("SET LOCAL {} = %s").format(psql.Identifier("default_transaction_read_only"))
                cr_svc.execute(set_query, ("on",))
                explain_prefix = psql.SQL("EXPLAIN (ANALYZE, BUFFERS) ")
                explain_query = explain_prefix + psql.SQL(query_text)
                cr_svc.execute(explain_query)
                plan = "\n".join([row[0] for row in cr_svc.fetchall()])
            finally:
                cr_svc.execute("ROLLBACK TO SAVEPOINT explain_sp")

            wizard = self.env["pg.explain.wizard"].create(
                {
                    "query": self.query,
                    "explain_plan": plan,
                }
            )

            return {
                "name": _("Query Explain Plan"),
                "type": "ir.actions.act_window",
                "res_model": "pg.explain.wizard",
                "res_id": wizard.id,
                "view_mode": "form",
                "target": "new",
            }
        except Exception as e:  # audit-ignore-catch-all
            _logger.warning("Failed to explain query: %s", e)
            raise UserError(_("Could not generate explain plan: %s") % str(e))


class PgExplainWizard(models.TransientModel):
    _name = "pg.explain.wizard"
    name = fields.Char(string="Name", default=lambda self: self._description)
    _description = "PostgreSQL Explain Wizard"

    query = fields.Text(string="SQL Query", readonly=True)
    explain_plan = fields.Text(string="Explain Plan", readonly=True)

    def action_close(self):
        return {"type": "ir.actions.act_window_close"}
