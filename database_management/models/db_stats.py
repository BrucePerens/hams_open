# -*- coding: utf-8 -*-
import logging
import os
import subprocess

from odoo import models, fields, api, tools, _
from odoo.exceptions import UserError

_logger = logging.getLogger(__name__)

class DatabaseTableStat(models.Model):
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
            pkg_name="postgresql-client" if cmd_name == "vacuumdb" else cmd_name
        )

    def action_vacuum_analyze(self):
        # [@ANCHOR: vacuum_analyze]
        # Tests [@ANCHOR: vacuum_analyze]
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
                    _logger.error("Vacuum failed for %s: %s", rec.table_name, res.stderr)
                    raise UserError(
                        _("Vacuum failed for %s: %s") % (rec.table_name, res.stderr)
                    )
            except subprocess.TimeoutExpired:
                raise UserError(_("Vacuum timed out for %s.") % rec.table_name)
            except Exception as e:
                raise UserError(_("Error executing vacuumdb: %s") % str(e))
        return True

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
            except Exception as e:
                _logger.warning("An error occurred: %s", e)


class DatabaseQueryStat(models.Model):
    # [@ANCHOR: db_slow_queries]
    # Tests [@ANCHOR: db_slow_queries]
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
        if self.env.cr.fetchone():
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
        else:
            self.env.cr.execute("""
                CREATE OR REPLACE VIEW database_query_stat AS (
                    SELECT 1 as id, 'pg_stat_statements extension not installed on host.' as query, 0 as calls, 0.0 as total_time, 0.0 as mean_time
                )
            """)


class DatabaseActivity(models.Model):
    # [@ANCHOR: db_active_sessions]
    # Tests [@ANCHOR: db_active_sessions]
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
