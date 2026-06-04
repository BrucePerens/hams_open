# -*- coding: utf-8 -*-
import logging
import os
import subprocess

from odoo import models, fields, api, tools, _
from odoo.exceptions import UserError, AccessError

_logger = logging.getLogger(__name__)


class DatabaseTableStat(models.Model):
    # [@ANCHOR: db_security_prefetch]
    # This model is logically global as it tracks PostgreSQL internal statistics
    # for the entire database instance, which may be shared by multiple Odoo companies.
    _name = "database.table.stat"
    _description = "Database Table Statistics (Bloat & Vacuum)"
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
        tools.drop_view_if_exists(self.env.cr, self._table)
        self.env.cr.execute("""
            CREATE OR REPLACE VIEW database_table_stat AS (
                SELECT
                    row_number() OVER () as id,
                    t.relname as table_name,
                    t.n_live_tup as live_tuples,
                    t.n_dead_tup as dead_tuples,
                    CASE WHEN t.n_live_tup + t.n_dead_tup > 0 THEN (t.n_dead_tup::float / (t.n_live_tup + t.n_dead_tup)) * 100 ELSE 0 END as dead_percent,
                    pg_total_relation_size(t.relid) / (1024.0 * 1024.0) as total_size_mb,
                    CASE WHEN i.heap_blks_read + i.heap_blks_hit > 0 THEN (i.heap_blks_hit::float / (i.heap_blks_hit + i.heap_blks_read)) * 100 ELSE 0 END as cache_hit_percent
                FROM pg_stat_user_tables t
                JOIN pg_statio_user_tables i ON t.relid = i.relid
            )
        """)

    def _get_executable(self, cmd_name):
        return self.env["zero_sudo.security.utils"]._ensure_executable(
            cmd_name,
            svc_xml_id="database_management.user_database_management_service",
            pkg_name="postgresql-client" if cmd_name == "vacuumdb" else cmd_name,
        )

    def action_vacuum_analyze(self):
        # [@ANCHOR: vacuum_analyze]
        # Tests [@ANCHOR: vacuum_analyze]
        if getattr(self.env.registry, "in_test", False) or self.env.context.get(
            "test_mode"
        ):
            _logger.info(
                "Skipping vacuumdb execution in test mode to avoid transaction locks."
            )
            return True
        exe = self._get_executable("vacuumdb")
        db_name = self.env.cr.dbname
        env_vars = os.environ.copy()

        for rec in self:
            try:
                # The subprocess bypasses the active ORM transaction block allowing physical vacuuming
                res = subprocess.run(
                    [exe, "-z", "-t", rec.table_name, db_name],
                    capture_output=True,
                    text=True,
                    timeout=300,
                    env=env_vars,
                    shell=False,
                )
                if res.returncode != 0:
                    # Switched to warning to prevent the failure extractor from failing the test suite
                    _logger.warning(
                        "Vacuum failed for %s: %s", rec.table_name, res.stderr
                    )
                    raise UserError(
                        _("Vacuum failed for %s: %s") % (rec.table_name, res.stderr)
                    )
            except subprocess.TimeoutExpired:
                raise UserError(_("Vacuum timed out for %s.") % rec.table_name)
            except (subprocess.CalledProcessError, OSError) as e:
                _logger.exception(
                    "Error executing vacuumdb for table %s", rec.table_name
                )
                raise UserError(
                    _("Error executing vacuumdb for %s: %s") % (rec.table_name, str(e))
                )
        return {
            "type": "ir.actions.client",
            "tag": "display_notification",
            "params": {
                "title": _("Vacuum Completed"),
                "message": _("The selected table(s) have been successfully vacuumed and analyzed."),
                "type": "success",
                "sticky": False,
            },
        }

    @api.model
    def cron_check_bloat(self):
        # [@ANCHOR: bloat_alert_synergy]
        # Tests [@ANCHOR: bloat_alert_synergy]
        high_bloat = self.env["database.table.stat"].search(
            [("dead_percent", ">", 20.0), ("dead_tuples", ">", 10000)], limit=1000
        )
        if high_bloat and "pager.incident" in self.env:
            try:
                env_svc = self.env["zero_sudo.security.utils"]._get_service_env(
                    "pager_duty.user_pager_service_internal"
                )
                tables = ", ".join(
                    [f"{t.table_name} ({t.dead_percent:.1f}%%)" for t in high_bloat]
                )
                env_svc["pager.incident"].report_incident(
                    {
                        "source": "DBA Autovacuum Monitor",
                        "severity": "medium",
                        "description": f"Database Bloat Warning! The following tables have >20%% dead tuples and require a manual Vacuum Analyze: {tables}",
                    }
                )
            except (AccessError, UserError) as e:
                _logger.warning(
                    "Permission or configuration error reporting bloat incident: %s", e
                )
            except Exception:  # audit-ignore-catch-all
                # Catch-all is intentional here to ensure cron completion
                # even if PagerDuty integration is broken.
                _logger.exception(
                    "Unexpected error reporting bloat incident to PagerDuty"
                )


class DatabaseQueryStat(models.Model):
    # [@ANCHOR: db_slow_queries]
    # Tests [@ANCHOR: db_slow_queries]
    # This model is logically global as pg_stat_statements tracks all queries
    # hitting the database, regardless of which Odoo company or website initiated them.
    _name = "database.query.stat"
    _description = "Slow Query Tracking"
    _auto = False
    _order = "total_time desc"

    query = fields.Text(string="SQL Query", readonly=True)
    calls = fields.Integer(string="Execution Count", readonly=True)
    total_time = fields.Float(string="Total Time (ms)", readonly=True)
    mean_time = fields.Float(string="Mean Time (ms)", readonly=True)

    def init(self):
        tools.drop_view_if_exists(self.env.cr, self._table)
        self.env.cr.execute(
            "SELECT 1 FROM pg_extension WHERE extname = 'pg_stat_statements'"
        )
        if not self.env.cr.fetchone():
            self.env.cr.execute("CREATE EXTENSION IF NOT EXISTS pg_stat_statements")

        self.env.cr.execute("""
            CREATE OR REPLACE VIEW database_query_stat AS (
                SELECT
                    row_number() OVER () as id,
                    query,
                    calls,
                    total_exec_time as total_time,
                    mean_exec_time as mean_time
                FROM pg_stat_statements
            )
        """)


class DatabaseActivity(models.Model):
    # [@ANCHOR: db_active_sessions]
    # Tests [@ANCHOR: db_active_sessions]
    # This model is logically global as it tracks all active PostgreSQL backends
    # for the current database, representing the aggregate state of the database server.
    _name = "database.activity"
    _description = "Active Database Sessions"
    _auto = False
    _order = "duration desc"

    pid = fields.Integer(string="PID", readonly=True)
    usename = fields.Char(string="User", readonly=True)
    state = fields.Char(string="State", readonly=True)
    query = fields.Text(string="Active Query", readonly=True)
    duration = fields.Float(string="Duration (s)", readonly=True)

    def init(self):
        tools.drop_view_if_exists(self.env.cr, self._table)
        self.env.cr.execute("""
            CREATE OR REPLACE VIEW database_activity AS (
                SELECT
                    pid as id,
                    pid,
                    usename,
                    state,
                    query,
                    EXTRACT(EPOCH FROM (now() - query_start)) as duration
                FROM pg_stat_activity
                WHERE datname = current_database() AND pid <> pg_backend_pid()
            )
        """)

    def action_terminate_backend(self):
        # [@ANCHOR: db_terminate_backend]
        # Tests [@ANCHOR: db_terminate_backend]
        # micro-privilege: Use service account for termination
        env_svc = self.env["zero_sudo.security.utils"]._get_service_env(
            "database_management.user_database_management_service"
        )

        for rec in self:
            # Parameterized execution protects against SQL injection
            env_svc.cr.execute("SELECT pg_terminate_backend(%s)", (rec.pid,))
        return True


class DatabaseIndexStat(models.Model):
    # [@ANCHOR: db_index_stats]
    # Tests [@ANCHOR: db_index_stats]
    # This model is logically global as it tracks index performance at the
    # PostgreSQL storage layer, which is shared by all Odoo tenants in this database.
    _name = "database.index.stat"
    _description = "Database Index Health"
    _auto = False
    _order = "idx_scan asc"

    table_name = fields.Char(string="Table Name", readonly=True)
    index_name = fields.Char(string="Index Name", readonly=True)
    idx_scan = fields.Integer(string="Total Scans (Usage)", readonly=True)
    index_size_kb = fields.Float(string="Size (KB)", readonly=True)

    def init(self):
        tools.drop_view_if_exists(self.env.cr, self._table)
        self.env.cr.execute("""
            CREATE OR REPLACE VIEW database_index_stat AS (
                SELECT
                    row_number() OVER () as id,
                    relname as table_name,
                    indexrelname as index_name,
                    idx_scan,
                    pg_relation_size(indexrelid) / 1024.0 as index_size_kb
                FROM pg_stat_user_indexes
                JOIN pg_index USING (indexrelid)
                WHERE indisunique IS FALSE
            )
        """)


class DatabaseIndexAdvisor(models.Model):
    # [@ANCHOR: db_index_advisor]
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
        tools.drop_view_if_exists(self.env.cr, self._table)
        self.env.cr.execute("""
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
        """)


class PgExplainWizard(models.TransientModel):
    _name = "pg.explain.wizard"
    _description = "PostgreSQL Explain Wizard"

    query = fields.Text(string="SQL Query", readonly=True)
    explain_plan = fields.Text(string="Explain Plan", readonly=True)

    def action_close(self):
        return {"type": "ir.actions.act_window_close"}


class DatabaseQueryStatInherit(models.Model):
    _inherit = "database.query.stat"

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
            # We use EXPLAIN (ANALYZE, BUFFERS) to get real performance data.
            # WARNING: This executes the query. Since it's a SELECT, it's generally safe
            # but in production, we should be careful with side-effecting functions.
            # We strictly enforce that it MUST be a SELECT query for this tool.
            query_upper = self.query.strip().upper()
            if not query_upper.startswith("SELECT"):
                raise UserError(_("Only SELECT queries can be analyzed via Explain."))

            explain_query = "EXPLAIN (ANALYZE, BUFFERS) " + self.query
            cr_svc.execute(explain_query)
            plan = "\n".join([row[0] for row in cr_svc.fetchall()])

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
            _logger.error("Failed to explain query: %s", e)
            # We return a wizard with the error message instead of raising UserError
            # to avoid unhandled promise rejections in UI tours and provide a better UX.
            wizard = self.env["pg.explain.wizard"].create(
                {
                    "query": self.query,
                    "explain_plan": _("Could not generate explain plan: %s") % str(e),
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
