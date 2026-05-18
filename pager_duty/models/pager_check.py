# -*- coding: utf-8 -*-
import uuid
import json
import os
import psutil
import subprocess
import logging

from odoo import models, fields, api, _
from odoo.exceptions import UserError
from odoo.addons.distributed_redis_cache.redis_cache import (
    distributed_cache,
    invalidate_model_cache,
)

_logger = logging.getLogger(__name__)

class PagerCheck(models.Model):
    _name = "pager.check"
    _description = "Graphical Pager Duty Check"
    _order = "name asc"

    name = fields.Char(string="Check Name / Source", required=True)
    status = fields.Selection(
        [
            ("passing", "Passing"),
            ("failing", "Failing"),
            ("maintenance", "Maintenance"),
        ],
        string="Status",
        default="passing",
        readonly=True,
    )
    last_run = fields.Datetime(string="Last Run", readonly=True)
    check_type = fields.Selection(
        [
            ("system", "System Resource (Disk/RAM/CPU/IO)"),
            ("dns", "DNS Resolution"),
            ("http", "HTTP(S) Endpoint"),
            ("http3", "HTTP/3 (QUIC) Endpoint"),
            ("tcp", "TCP Socket"),
            ("udp", "UDP Datagram"),
            ("redis", "Redis Server (Ping)"),
            ("rabbitmq", "RabbitMQ Server (AMQP)"),
            ("xmlrpc", "XML-RPC Method"),
            ("jsonrpc", "JSON-RPC Method"),
            ("postgres", "PostgreSQL DB (Ping)"),
            ("log", "Log File Tail"),
            ("ssl", "SSL/TLS Certificate Expiry"),
            ("anomaly", "Anomaly Detection (SQL Baseline)"),
            ("synthetic", "Synthetic Journey (Script)"),
            ("certbot", "Certbot Readiness / Dry-Run"),
            ("pg_dump", "PostgreSQL Backup (Dry-Run)"),
            ("nginx", "Nginx Config (Syntax Check)"),
            ("logrotate", "Logrotate (Dry-Run)"),
            ("cloudflared", "Cloudflare Tunnel (Pre-Flight)"),
            ("smtp_dryrun", "SMTP Login (Dry-Run)"),
            ("icmp", "ICMP Ping"),
            ("heartbeat", "Heartbeat (Push Monitor)"),
            ("docker", "Docker Container Health"),
            ("playwright", "Playwright Synthetic Journey"),
            ("bash", "Sandboxed Bash Script"),
            ("executable", "Sandboxed Arbitrary Executable"),
            ("smart", "SMART Disk Health"),
            ("file_absent", "File Must Not Exist (e.g. reboot-required)"),
            ("memcached", "Memcached Server (Ping)"),
            ("ssh", "SSH Handshake"),
            ("systemd", "Systemd Service Status"),
            ("ftp", "FTP Login"),
            ("imap", "IMAP Login"),
            ("pop3", "POP3 Login"),
            ("mysql", "MySQL/MariaDB DB (Ping)"),
            ("snmp", "SNMP Get"),
            ("ldap", "LDAP Server"),
            ("ntp", "NTP Server"),
            ("load", "System Load Average"),
        ],
        string="Monitor Type",
        required=True,
    )

    snmp_community = fields.Char(string="SNMP Community", default="public")
    snmp_oid = fields.Char(string="SNMP OID")
    partition = fields.Char(
        string="Disk Partition",
        default="/",
        help="Specific mount point to check for disk usage.",
    )
    ignored_services = fields.Text(
        string="Ignored Services",
        help="Comma-separated list of systemd services to ignore when monitoring all failures (e.g., 'fwupd-refresh.service').",
    )
    warning_threshold = fields.Integer(string="Warning Threshold %")

    target = fields.Char(
        string="Target (Host/URL/File)",
        help="Prefix with ENV: to inject environment variables.",
    )
    port = fields.Integer(string="Port")
    payload_send = fields.Char(string="Send Payload (String)")
    payload_send_hex = fields.Char(string="Send Payload (Hex)")
    payload_expect = fields.Char(string="Expect Output")

    dbname = fields.Char(string="DB Name")
    dbuser = fields.Char(string="Username (DB/SMTP)")
    dbpass = fields.Char(string="Password (DB/SMTP)", password=True)
    query = fields.Text(string="SQL Query (Returns Integer)")
    script = fields.Char(string="Shell Script Command")
    rpc_method = fields.Char(string="RPC Method", help="e.g. execute_kw")
    rpc_params = fields.Text(
        string="RPC Params (JSON Array/Dict)",
        help="e.g. ['db', 2, 'pass', 'res.partner', 'search', [[]]]",
    )

    regex = fields.Char(string="Regex Pattern")
    critical_threshold = fields.Integer(string="Critical Threshold %")
    interval = fields.Integer(string="Polling Interval (sec)", default=60)
    grace_period = fields.Integer(
        string="Startup Grace Period (sec)",
        default=0,
        help="Seconds to wait after daemon startup before reporting failures.",
    )
    active = fields.Boolean(default=True)

    parent_check_id = fields.Many2one(
        "pager.check",
        string="Parent Check (Dependency)",
        help="If parent fails, this check is suppressed.",
    )
    maintenance_start = fields.Datetime(string="Maintenance Start")
    maintenance_end = fields.Datetime(string="Maintenance End")

    code_payload = fields.Text(string="Script Code (Python/Bash)")
    executable_path = fields.Char(
        string="Executable Path/Name", help="e.g., ./my_binary"
    )
    executable_args = fields.Char(
        string="Executable Arguments", help="e.g., --check --verbose"
    )
    sandbox_downloads = fields.Text(
        string="Sandbox Downloads",
        help="Provide one entry per line in the format: URL, then SHA-256, then Filename. Downloaded into the sandbox before execution.",
    )
    sandbox_network_access = fields.Selection(
        [
            ("loopback", "Loop-back network only (more secure)"),
            ("full", "Full network access"),
        ],
        string="Network Access",
        default="loopback",
        help="Full network access allows reaching the internet and LAN (SSRF risk). Loop-back restricts the sandbox to local interfaces.",
    )

    auto_remediate_script = fields.Char(
        string="Auto-Remediation Script",
        help="Executed locally by daemon if check fails (e.g., /opt/reboot.sh)",
    )
    comment = fields.Text(
        string="Comments",
        help="Multi-line comments for documentation. This will appear natively in the JSON configuration.",
    )

    heartbeat_uuid = fields.Char(
        string="Heartbeat UUID", default=lambda self: str(uuid.uuid4()), readonly=True
    )
    last_heartbeat = fields.Datetime(string="Last Heartbeat", readonly=True)

    _name_uniq = models.Constraint("UNIQUE(name)", "The check name must be unique!")
    _uuid_uniq = models.Constraint(
        "UNIQUE(heartbeat_uuid)", "The heartbeat UUID must be unique!"
    )

    _name_not_empty = models.Constraint(
        "CHECK(LENGTH(TRIM(name)) > 0)",
        "The check name cannot be empty or just spaces.",
    )
    _interval_positive = models.Constraint(
        "CHECK(interval > 0)",
        "The polling interval must be strictly greater than zero.",
    )
    _grace_period_non_negative = models.Constraint(
        "CHECK(grace_period >= 0)", "The grace period cannot be negative."
    )

    @api.model
    @distributed_cache()
    def _get_check_id_by_uuid(self, hb_uuid, override_svc_uid=None):
        if not hb_uuid:
            return False
        svc_uid = override_svc_uid or self.env[
            "zero_sudo.security.utils"
        ]._get_service_uid("pager_duty.user_pager_service_internal")
        check = (
            self.env["pager.check"]
            .with_user(svc_uid)
            .search([("heartbeat_uuid", "=", hb_uuid)], limit=1)
        )
        return check.id if check else False

    def write(self, vals):
        res = super(
            PagerCheck, self.with_context(mail_notrack=True)
        ).write(vals)
        invalidate_model_cache(self.env, self._name)
        payload = json.dumps({"model": self._name})
        self.env.cr.execute(
            "SELECT pg_notify(%s, %s)", ("distributed_cache_invalidation", payload)
        )
        return res

    def unlink(self):
        invalidate_model_cache(self.env, self._name)
        payload = json.dumps({"model": self._name})
        self.env.cr.execute(
            "SELECT pg_notify(%s, %s)", ("distributed_cache_invalidation", payload)
        )
        return super(
            PagerCheck, self.with_context(mail_notrack=True)
        ).unlink()

    @api.model
    def _valid_field_parameter(self, field, name):
        return name == "password" or super()._valid_field_parameter(field, name)

    @api.model
    def rpc_ensure_executable(self, cmd_name):
        try:
            path = self.env["binary.manifest"].ensure_executable(cmd_name)
            return {"status": "ok", "path": path}
        except (ValueError, FileNotFoundError, PermissionError) as e:
            _logger.warning("Executable provisioning failed for %s: %s", cmd_name, e)
            return {"status": "error", "message": str(e)}
        except Exception as e: # audit-ignore-catch-all
            _logger.error("Unexpected error during executable provisioning for %s: %s", cmd_name, e)
            return {"status": "error", "message": _("Internal server error.")}

    @api.model
    def check_heartbeat_rpc(self, hb_uuid, interval_sec):
        check_id = self._get_check_id_by_uuid(hb_uuid)
        if not check_id:
            return False
        check = self.env["pager.check"].browse(check_id)
        if not check.last_heartbeat:
            return False
        delta = (fields.Datetime.now() - check.last_heartbeat).total_seconds()
        return delta <= interval_sec

    @api.model
    def _get_config_path(self):
        base_dir = os.path.abspath(
            os.path.join(os.path.dirname(__file__), "..", "daemon")
        )
        return os.path.join(base_dir, "pager_config.json")

    def action_pull_from_json(self):
        path = self._get_config_path()
        if not os.path.exists(path):
            raise UserError(_("No JSON configuration found at %s") % path)
        try:
            with open(path, "r", encoding="utf-8") as f:
                data = json.load(f)
        except json.JSONDecodeError as e:
            _logger.error("JSON parse error at %s: %s", path, e)
            raise UserError(_("Invalid JSON Format in %s: %s") % (path, str(e)))
        except IOError as e:
            _logger.error("IO error reading %s: %s", path, e)
            raise UserError(_("Failed to read configuration file: %s") % str(e))

        self.env["pager.check"].search([], limit=1000).unlink()

        # Parse Log Analyzer Config
        if "log_analyzer" in data:
            log_config = data["log_analyzer"]
            self.env["pager.log.file"].search([], limit=1000).unlink()
            self.env["pager.log.pattern"].search([], limit=1000).unlink()

            for fp in log_config.get("files", []):
                self.env["pager.log.file"].create({"filepath": fp})
            for pat in log_config.get("patterns", []):
                self.env["pager.log.pattern"].create(
                    {
                        "name": pat.get("name", "Unnamed"),
                        "regex": pat.get("regex", ""),
                        "severity": pat.get("severity", "high"),
                    }
                )

        checks = data.get("checks", []) if isinstance(data, dict) else []
        for c in checks:
            self.env["pager.check"].create(
                {
                    "name": c.get("name", "Unnamed"),
                    "check_type": c.get("type", "system"),
                    "target": c.get("target", ""),
                    "port": c.get("port", 0),
                    "payload_send": c.get("send", ""),
                    "payload_send_hex": c.get("send_hex", ""),
                    "payload_expect": c.get("expect", ""),
                    "dbname": c.get("dbname", ""),
                    "dbuser": c.get("user", ""),
                    "dbpass": c.get("password", ""),
                    "query": c.get("query", ""),
                    "script": c.get("script", ""),
                    "rpc_method": c.get("rpc_method", ""),
                    "rpc_params": c.get("rpc_params", ""),
                    "regex": c.get("regex", ""),
                    "critical_threshold": c.get("critical", 0),
                    "warning_threshold": c.get("warning", 0),
                    "snmp_community": c.get("snmp_community", ""),
                    "snmp_oid": c.get("snmp_oid", ""),
                    "partition": c.get("partition", "/"),
                    "ignored_services": c.get("ignored_services", ""),
                    "interval": c.get("interval", 60),
                    "grace_period": c.get("grace", 0),
                    "auto_remediate_script": c.get("remediate", ""),
                    "comment": c.get("comment", ""),
                    "code_payload": c.get("code_payload", ""),
                    "executable_path": c.get("executable_path", ""),
                    "executable_args": c.get("executable_args", ""),
                    "sandbox_downloads": c.get("sandbox_downloads", ""),
                    "sandbox_network_access": c.get(
                        "sandbox_network_access", "loopback"
                    ),
                    "maintenance_start": c.get("maint_start"),
                    "maintenance_end": c.get("maint_end"),
                }
            )

        all_checks = self.env["pager.check"].search([], limit=1000)
        name_to_id = {rec.name: rec.id for rec in all_checks}
        for c in checks:
            if (
                c.get("parent")
                and c.get("parent") in name_to_id
                and c.get("name") in name_to_id
            ):
                check_rec = next(
                    (r for r in all_checks if r.id == name_to_id[c.get("name")]), None
                )
                if check_rec:
                    check_rec.write({"parent_check_id": name_to_id[c.get("parent")]})

        return {
            "type": "ir.actions.client",
            "tag": "display_notification",
            "params": {
                "title": _("Import Successful"),
                "message": _("Database checks updated from JSON file."),
                "type": "success",
                "sticky": False,
            },
        }

    def action_push_to_json(self):
        # [@ANCHOR: generalized_pager_config]
        checks = self.env["pager.check"].search([("active", "=", True)], limit=1000)
        check_list = []
        for c in checks:
            d = {
                "name": c.name,
                "type": c.check_type,
                "target": c.target,
                "interval": c.interval,
            }
            if c.port:
                d["port"] = c.port
            if c.payload_send:
                d["send"] = c.payload_send
            if c.payload_send_hex:
                d["send_hex"] = c.payload_send_hex
            if c.payload_expect:
                d["expect"] = c.payload_expect
            if c.dbname:
                d["dbname"] = c.dbname
            if c.dbuser:
                d["user"] = c.dbuser
            if c.dbpass:
                d["password"] = c.dbpass
            if c.query:
                d["query"] = c.query
            if c.script:
                d["script"] = c.script
            if c.rpc_method:
                d["rpc_method"] = c.rpc_method
            if c.rpc_params:
                d["rpc_params"] = c.rpc_params
            if c.regex:
                d["regex"] = c.regex
            if c.critical_threshold:
                d["critical"] = c.critical_threshold
            if c.warning_threshold:
                d["warning"] = c.warning_threshold
            if c.snmp_community:
                d["snmp_community"] = c.snmp_community
            if c.snmp_oid:
                d["snmp_oid"] = c.snmp_oid
            if c.partition and c.partition != "/":
                d["partition"] = c.partition
            if c.ignored_services:
                d["ignored_services"] = c.ignored_services
            if c.grace_period:
                d["grace"] = c.grace_period
            if c.parent_check_id:
                d["parent"] = c.parent_check_id.name
            if c.maintenance_start:
                d["maint_start"] = c.maintenance_start.strftime("%Y-%m-%d %H:%M:%S")
            if c.maintenance_end:
                d["maint_end"] = c.maintenance_end.strftime("%Y-%m-%d %H:%M:%S")
            if c.auto_remediate_script:
                d["remediate"] = c.auto_remediate_script
            if c.comment:
                d["comment"] = c.comment
            if c.check_type == "heartbeat":
                d["uuid"] = c.heartbeat_uuid
            if c.code_payload:
                d["code_payload"] = c.code_payload
            if c.executable_path:
                d["executable_path"] = c.executable_path
            if c.executable_args:
                d["executable_args"] = c.executable_args
            if c.sandbox_downloads:
                d["sandbox_downloads"] = c.sandbox_downloads
            if c.sandbox_network_access:
                d["sandbox_network_access"] = c.sandbox_network_access
            check_list.append(d)

        json_dict = {"checks": check_list}

        log_files = (
            self.env["pager.log.file"]
            .search([("active", "=", True)], limit=1000)
            .mapped("filepath")
        )
        log_patterns = self.env["pager.log.pattern"].search(
            [("active", "=", True)], limit=1000
        )

        json_dict["log_analyzer"] = {
            "files": log_files,
            "patterns": [
                {"name": p.name, "regex": p.regex, "severity": p.severity}
                for p in log_patterns
            ],
        }
        json_dict["odoo_database"] = self.env.cr.dbname

        json_content = json.dumps(json_dict, indent=2)
        path = self._get_config_path()
        os.makedirs(os.path.dirname(path), exist_ok=True)
        with open(path, "w", encoding="utf-8") as f:
            f.write(json_content)

        return {
            "type": "ir.actions.client",
            "tag": "display_notification",
            "params": {
                "title": _("Export Successful"),
                "message": _("Checks exported to JSON daemon configuration."),
                "type": "success",
                "sticky": False,
            },
        }

    @api.model
    def _run_autodiscovery(self):
        """Scans the host OS and systemd to build an optimal monitoring baseline."""
        checks = []

        # 1. Base System Resources & DNS
        checks.extend(
            [
                {
                    "name": "System Memory",
                    "check_type": "system",
                    "target": "memory",
                    "critical_threshold": 90,
                    "interval": 60,
                    "comment": "Autodiscovered RAM monitor",
                },
                {
                    "name": "System CPU",
                    "check_type": "system",
                    "target": "cpu",
                    "critical_threshold": 90,
                    "interval": 60,
                    "comment": "Autodiscovered CPU monitor",
                },
                {
                    "name": "System Load",
                    "check_type": "load",
                    "critical_threshold": 10,
                    "interval": 60,
                    "comment": "Autodiscovered system load monitor",
                },
                {
                    "name": "Systemd Failed Services Tracker",
                    "check_type": "systemd",
                    "target": "*",
                    "ignored_services": "fwupd-refresh.service, NetworkManager-wait-online.service",
                    "interval": 300,
                    "comment": "Autodiscovered global systemd failure tracker",
                },
                {
                    "name": "IPv4 Root DNS Reachability",
                    "check_type": "synthetic",
                    "script": "dig -4 +time=3 +tries=1 @198.41.0.4 . NS",
                    "interval": 300,
                    "comment": "Verifies IPv4 network routing to root DNS servers",
                },
                {
                    "name": "IPv6 Root DNS Reachability",
                    "check_type": "synthetic",
                    "script": "dig -6 +time=3 +tries=1 @2001:503:ba3e::2:30 . NS",
                    "interval": 300,
                    "comment": "Verifies IPv6 network routing to root DNS servers",
                },
            ]
        )

        # 2. Physical Disks
        try:
            for p in psutil.disk_partitions(all=False):
                if p.fstype in ("ext4", "xfs", "btrfs", "zfs", "vfat"):
                    checks.append(
                        {
                            "name": f"Disk Space ({p.mountpoint})",
                            "check_type": "system",
                            "target": "disk",
                            "partition": p.mountpoint,
                            "critical_threshold": 90,
                            "interval": 300,
                            "comment": f"Autodiscovered disk space monitor for {p.mountpoint}",
                        }
                    )
        except Exception as e: # audit-ignore-catch-all
            _logger.warning("An error occurred getting disk partitions: %s", e)

        # 3. Common Services
        try:
            res = subprocess.run(
                [
                    "systemctl",
                    "list-units",
                    "--type=service",
                    "--state=active",
                    "--no-legend",
                    "--plain",
                ],
                capture_output=True,
                text=True,
            )
            active_services = res.stdout

            if "postgresql.service" in active_services:
                checks.append(
                    {
                        "name": "PostgreSQL DB (Ping)",
                        "check_type": "postgres",
                        "target": "postgres",
                        "port": 5432,
                        "dbname": "postgres",
                        "dbuser": "postgres",
                        "interval": 60,
                        "comment": "Autodiscovered PostgreSQL instance",
                    }
                )
            if (
                "redis-server.service" in active_services
                or "redis.service" in active_services
            ):
                checks.append(
                    {
                        "name": "Redis Ping",
                        "check_type": "redis",
                        "target": "redis",
                        "port": 6379,
                        "interval": 60,
                        "comment": "Autodiscovered Redis instance",
                    }
                )
            if "rabbitmq-server.service" in active_services:
                checks.append(
                    {
                        "name": "RabbitMQ AMQP",
                        "check_type": "rabbitmq",
                        "target": "rabbitmq",
                        "port": 5672,
                        "interval": 60,
                        "comment": "Autodiscovered RabbitMQ instance",
                    }
                )
            if "nginx.service" in active_services:
                checks.append(
                    {
                        "name": "Nginx Config Readiness",
                        "check_type": "nginx",
                        "interval": 3600,
                        "comment": "Autodiscovered Nginx syntax check",
                    }
                )
            if "docker.service" in active_services:
                checks.append(
                    {
                        "name": "Docker Daemon",
                        "check_type": "systemd",
                        "target": "docker.service",
                        "interval": 60,
                        "comment": "Autodiscovered Docker daemon monitor",
                    }
                )
        except Exception as e: # audit-ignore-catch-all
            _logger.warning("An error occurred interacting with systemd: %s", e)

        # 4. Odoo Web Server
        checks.append(
            {
                "name": "WSGI HTTP Ping",
                "check_type": "http",
                "target": "http://odoo:8069/api/v1/pager/ping",
                "payload_expect": '{"status": "ok"}',
                "interval": 60,
                "grace_period": 120,
                "comment": "Autodiscovered local Odoo WSGI loopback ping",
            }
        )

        existing_names = set(
            self.env["pager.check"].search([], limit=5000).mapped("name")
        )
        new_checks = []
        for c in checks:
            if c["name"] not in existing_names:
                new_checks.append(c)
                existing_names.add(c["name"])

        if new_checks:
            self.env["pager.check"].create(new_checks)
            self.action_push_to_json()

    def action_autodiscover(self):
        self._run_autodiscovery()
        return {
            "type": "ir.actions.client",
            "tag": "display_notification",
            "params": {
                "title": _("Autodiscovery Complete"),
                "message": _(
                    "Scanned the system and injected optimal monitoring checks. Daemon configuration updated."
                ),
                "type": "success",
                "sticky": False,
            },
        }

    def action_trigger_check(self):
        return {
            "type": "ir.actions.client",
            "tag": "display_notification",
            "params": {
                "title": _("Daemon Notified"),
                "message": _(
                    "The external SRE daemon operates asynchronously outside of Odoo. It will execute this check on its next polling cycle."
                ),
                "type": "info",
                "sticky": False,
            },
        }
