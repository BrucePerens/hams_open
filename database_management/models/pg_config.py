# This software is distributed under the terms of the Affero General Public License (AGPL-3).

# -*- coding: utf-8 -*-
import re
from odoo import models, fields, _
from odoo.exceptions import UserError
from psycopg2 import sql
import odoo.tools.sql
import contextlib


class DatabasePgSetting(models.Model):
    # [@ANCHOR: db_settings_audit]

    # Tests [@ANCHOR: db_settings_audit]
    # This model is logically global as it tracks the overall PostgreSQL configuration
    # settings for the database cluster.
    _name = "database.pg.setting"
    _description = "PostgreSQL Configuration Parameter"
    _auto = False
    _order = "category asc, name asc"

    name = fields.Char(string="Parameter", readonly=True)
    setting = fields.Char(string="Current Value", readonly=True)
    unit = fields.Char(string="Unit", readonly=True)
    category = fields.Char(string="Category", readonly=True)
    short_desc = fields.Char(string="Description", readonly=True)
    context = fields.Char(string="Context", readonly=True)
    pending_restart = fields.Boolean(string="Pending Restart", readonly=True)

    def init(self):
        odoo.tools.sql.drop_view_if_exists(self.env.cr, self._table)
        self.env.cr.execute(
            """
            CREATE OR REPLACE VIEW database_pg_setting AS (
                SELECT
                    row_number() OVER (ORDER BY name) as id,
                    name,
                    setting,
                    unit,
                    category,
                    short_desc,
                    context,
                    pending_restart
                FROM pg_settings
            )
        """
        )


class PgOptimizeWizard(models.TransientModel):
    _name = "pg.optimize.wizard"
    _description = "PostgreSQL Optimization Wizard"
    name = fields.Char(string="Name", default=lambda self: self._description)

    ram_gb = fields.Integer(string="Total Server RAM (GB)", required=True, default=8)
    cpu_cores = fields.Integer(string="Total CPU Cores", required=True, default=4)
    storage_type = fields.Selection(
        [("ssd", "SSD / NVMe"), ("hdd", "Traditional HDD")],
        required=True,
        default="ssd",
    )
    max_connections = fields.Integer(
        string="Max Connections", required=True, default=200
    )

    def action_apply_optimizations(self):
        # [@ANCHOR: pg_optimize_wizard]

        # Tests [@ANCHOR: pg_optimize_wizard]
        if self.ram_gb <= 0 or self.cpu_cores <= 0 or self.max_connections <= 0:
            raise UserError(_("RAM, CPU and Max Connections must be greater than zero."))


        # Standard DBA Tuning Algorithms
        shared_buffers_mb = int((self.ram_gb * 1024) * 0.25)
        effective_cache_mb = int((self.ram_gb * 1024) * 0.75)
        maintenance_work_mem_mb = min(1024, int((self.ram_gb * 1024) * 0.05))
        work_mem_mb = max(4, int(((self.ram_gb * 1024) * 0.25) / self.max_connections))
        random_page_cost = 1.1 if self.storage_type == "ssd" else 4.0
        max_worker_processes = self.cpu_cores
        max_parallel_workers = max(2, int(self.cpu_cores / 2))

        settings = {
            "shared_buffers": f"{shared_buffers_mb}MB",
            "effective_cache_size": f"{effective_cache_mb}MB",
            "maintenance_work_mem": f"{maintenance_work_mem_mb}MB",
            "work_mem": f"{work_mem_mb}MB",
            "max_worker_processes": str(max_worker_processes),
            "max_parallel_workers_per_gather": str(max_parallel_workers),
            "max_parallel_workers": str(max_worker_processes),
            "random_page_cost": str(random_page_cost),
            "max_connections": str(self.max_connections),
        }

        with self.env.registry.cursor() as cr:
            cr.autocommit(True)
            for param, val in settings.items():
                # CRITICAL: AST-compliant parameterized execution for ALTER SYSTEM
                query = sql.SQL("ALTER SYSTEM SET {} = {}").format(
                    sql.Identifier(param), sql.Literal(val)
                )
                cr.execute(query)

            cr.execute("SELECT pg_reload_conf()")

        return {
            "type": "ir.actions.client",
            "tag": "display_notification",
            "params": {
                "title": _("Optimizations Applied"),
                "message": _(
                    "Configuration written to postgresql.auto.conf. NOTE: Parameters like shared_buffers and max_connections require a full PostgreSQL service restart to take effect."
                ),
                "type": "warning",
                "sticky": True,
            },
        }


class PgHaWizard(models.TransientModel):
    _name = "pg.ha.wizard"
    _description = "High Availability Failover Wizard"
    name = fields.Char(string="Name", default=lambda self: self._description)

    cluster_name = fields.Char(
        string="Cluster Name", required=True, default="hams_cluster"
    )
    primary_ip = fields.Char(
        string="Primary Node IP", required=True, default="10.0.0.1"
    )
    secondary_ip = fields.Char(
        string="Secondary Node IP", required=True, default="10.0.0.2"
    )
    etcd_hosts = fields.Char(
        string="Etcd Hosts",
        required=True,
        default="etcd:2379",
        help="Comma-separated list of etcd hosts (e.g., 10.0.0.1:2379,10.0.0.2:2379)",
    )
    replication_user = fields.Char(
        string="Replication User", required=True, default="replicator"
    )
    replication_pass = fields.Char(
        string="Replication Password",
        required=True,
        default="SecureRepPass123!",
    )
    superuser_user = fields.Char(
        string="Superuser Name", required=True, default="postgres"
    )

    state = fields.Selection(
        [("input", "Input"), ("generated", "Generated")], default="input"
    )
    patroni_primary = fields.Text(string="Primary Patroni YAML", readonly=True)
    patroni_secondary = fields.Text(string="Secondary Patroni YAML", readonly=True)
    pgbouncer_ini = fields.Text(string="PgBouncer INI", readonly=True)

    def _get_executable(self, cmd_name):
        pkg_map = {
            "patroni": "patroni",
            "pgbouncer": "pgbouncer",
            "etcd": "etcd",
        }
        return self.env["zero_sudo.security.utils"]._ensure_executable(
            cmd_name,
            svc_xml_id="database_management.user_database_management_service",
            pkg_name=pkg_map.get(cmd_name, cmd_name),
        )

    def _validate_inputs(self):
        ip_pattern = re.compile(r"^\d{1,3}\.\d{1,3}\.\d{1,3}\.\d{1,3}$")
        if not self.primary_ip or not ip_pattern.match(self.primary_ip):
            raise UserError(_("Invalid Primary Node IP format."))
        if not self.secondary_ip or not ip_pattern.match(self.secondary_ip):
            raise UserError(_("Invalid Secondary Node IP format."))
        if not self.replication_pass or len(self.replication_pass) < 8:
            raise UserError(
                _("Replication Password must be at least 8 characters long.")
            )

        alnum_pattern = re.compile(r"^[a-zA-Z0-9_]+$")
        if not self.cluster_name or not alnum_pattern.match(self.cluster_name):
            raise UserError(_("Invalid cluster name. Must be alphanumeric."))
        if not self.superuser_user or not alnum_pattern.match(self.superuser_user):
            raise UserError(_("Invalid superuser name. Must be alphanumeric."))
        if not self.replication_user or not alnum_pattern.match(self.replication_user):
            raise UserError(_("Invalid replication user name. Must be alphanumeric."))

    def action_generate(self):
        # [@ANCHOR: pg_ha_wizard]

        # Tests [@ANCHOR: pg_ha_wizard]
        self._validate_inputs()

        # Check required binaries before generating config
        self._get_executable("patroni")
        self._get_executable("pgbouncer")
        self._get_executable("etcd")

        etcd_config = (
            "host: " + self.etcd_hosts
            if "," not in self.etcd_hosts
            else "hosts: [" + self.etcd_hosts + "]"
        )

        self.patroni_primary = f"""scope: {self.cluster_name}
namespace: /db/
name: node1
restapi:
  listen: {self.primary_ip}:8008
  connect_address: {self.primary_ip}:8008
etcd:
  {etcd_config}
bootstrap:
  dcs:
    ttl: 30
    loop_wait: 10
  initdb:
    - auth-host: md5
    - auth-local: trust
    - encoding: UTF8
    - data-checksums
postgresql:
  listen: {self.primary_ip}:5432
  connect_address: {self.primary_ip}:5432
  data_dir: /var/lib/postgresql/data
  authentication:
    replication:
      username: {self.replication_user}
      password: {self.replication_pass}
    superuser:
      username: {self.superuser_user}
      password: {self.replication_pass}"""

        self.patroni_secondary = f"""scope: {self.cluster_name}
namespace: /db/
name: node2
restapi:
  listen: {self.secondary_ip}:8008
  connect_address: {self.secondary_ip}:8008
etcd:
  {etcd_config}
postgresql:
  listen: {self.secondary_ip}:5432
  connect_address: {self.secondary_ip}:5432
  data_dir: /var/lib/postgresql/data
  authentication:
    replication:
      username: {self.replication_user}
      password: {self.replication_pass}
    superuser:
      username: {self.superuser_user}
      password: {self.replication_pass}"""

        self.pgbouncer_ini = f"""[databases]
* = host={self.primary_ip} port=5432 auth_user=pgbouncer

[pgbouncer]
listen_port = 6432
listen_addr = *
auth_type = md5
auth_file = /etc/pgbouncer/userlist.txt
pool_mode = transaction
max_client_conn = 1000
default_pool_size = 20
"""

        self.state = "generated"
        return {
            "type": "ir.actions.act_window",
            "res_model": "pg.ha.wizard",
            "res_id": self.id,
            "view_mode": "form",
            "target": "new",
        }
