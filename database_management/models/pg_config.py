from odoo import models, fields, tools, _
from odoo.exceptions import UserError
import logging
from psycopg2 import sql
import shutil

_logger = logging.getLogger(__name__)

class DatabasePgSetting(models.Model):
    # [@ANCHOR: db_security_prefetch]
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
        tools.drop_view_if_exists(self.env.cr, self._table)
        self.env.cr.execute("""
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
        """)


class PgOptimizeWizard(models.TransientModel):
    _name = "pg.optimize.wizard"
    _description = "PostgreSQL Optimization Wizard"

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
        if self.ram_gb <= 0 or self.cpu_cores <= 0:
            raise UserError(_("RAM and CPU must be greater than zero."))

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

        for param, val in settings.items():
            # CRITICAL: AST-compliant parameterized execution for ALTER SYSTEM
            query = sql.SQL("ALTER SYSTEM SET {} = {}").format(
                sql.Identifier(param), sql.Literal(val)
            )
            self.env.cr.execute(query)

        self.env.cr.execute("SELECT pg_reload_conf()")

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

    primary_ip = fields.Char(
        string="Primary Node IP", required=True, default="10.0.0.1"
    )
    secondary_ip = fields.Char(
        string="Secondary Node IP", required=True, default="10.0.0.2"
    )
    replication_pass = fields.Char(
        string="Replication Password", required=True, default="SecureRepPass123!"
    )

    state = fields.Selection(
        [("input", "Input"), ("generated", "Generated")], default="input"
    )
    patroni_primary = fields.Text(string="Primary Patroni YAML", readonly=True)
    patroni_secondary = fields.Text(string="Secondary Patroni YAML", readonly=True)
    pgbouncer_ini = fields.Text(string="PgBouncer INI", readonly=True)

    def _get_executable(self, cmd_name):

        path = shutil.which(cmd_name)
        if path:
            return path

        if cmd_name == "etcd":
            svc_uid = self.env["zero_sudo.security.utils"]._get_service_uid(
                "database_management.user_database_management_service"
            )
            return (
                self.env["binary.manifest"]
                .with_user(svc_uid)
                .ensure_executable("etcd")
            )

        pkg_map = {"patroni": "patroni", "pgbouncer": "pgbouncer"}
        pkg = pkg_map.get(cmd_name, cmd_name)
        raise UserError(
            _(
                "Missing dependency: '%s'. Please install via OS package manager (e.g., 'apt-get install %s')."
            )
            % (cmd_name, pkg)
        )

    def action_generate(self):
        # [@ANCHOR: pg_ha_wizard]
        self._get_executable("etcd")
        self._get_executable("patroni")
        self._get_executable("pgbouncer")

        self.patroni_primary = f"""scope: hams_cluster
namespace: /db/
name: node1
restapi:
  listen: {self.primary_ip}:8008
  connect_address: {self.primary_ip}:8008
etcd:
  host: 127.0.0.1:2379
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
      username: replicator
      password: {self.replication_pass}
    superuser:
      username: postgres
      password: {self.replication_pass}"""

        self.patroni_secondary = f"""scope: hams_cluster
namespace: /db/
name: node2
restapi:
  listen: {self.secondary_ip}:8008
  connect_address: {self.secondary_ip}:8008
etcd:
  host: 127.0.0.1:2379
postgresql:
  listen: {self.secondary_ip}:5432
  connect_address: {self.secondary_ip}:5432
  data_dir: /var/lib/postgresql/data
  authentication:
    replication:
      username: replicator
      password: {self.replication_pass}
    superuser:
      username: postgres
      password: {self.replication_pass}"""

        self.pgbouncer_ini = """[databases]
* = host=127.0.0.1 port=5432 auth_user=pgbouncer

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
