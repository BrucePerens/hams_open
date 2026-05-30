# -*- coding: utf-8 -*-
import logging
import json
import os
import datetime
import shutil
import pika
import odoo
from odoo import models, fields, api, _
from odoo.exceptions import UserError, AccessError
from .utils import validate_backup_path

from cryptography.fernet import Fernet


class BackupConfig(models.Model):
    _name = "backup.config"
    _description = "Backup Configuration"
    _inherit = ["mail.thread"]

    name = fields.Char(string="Name", required=True)
    website_id = fields.Many2one("website", string="Website")
    company_id = fields.Many2one("res.company", string="Company", default=lambda self: self.env.company)
    engine = fields.Selection(
        [("kopia", "Kopia"), ("pgbackrest", "pgBackRest")], required=True
    )
    target_path = fields.Char(
        string="Target / Stanza",
        required=True,
        help="Repository path for Kopia, or Stanza name for pgBackRest.",
    )
    minimum_size_mb = fields.Integer(
        string="Minimum Size (MB)",
        default=0,
        help="Triggers a Pager Duty alert if a new snapshot is smaller than this.",
    )

    kopia_password_crypt = fields.Char(
        string="Encrypted Kopia Password", groups="backup_management.group_backup_admin"
    )
    kopia_password = fields.Char(
        string="Kopia Password",
        compute="_compute_kopia_password",
        inverse="_inverse_kopia_password",
        groups="backup_management.group_backup_admin",
    )

    storage_type = fields.Selection(
        [("local", "Local Directory"), ("s3", "AWS S3"), ("b2", "Backblaze B2")],
        default="local",
        string="Storage Type",
        required=True,
    )
    bucket_name = fields.Char(string="Bucket Name")
    endpoint_url = fields.Char(string="Endpoint URL")
    access_key = fields.Char(
        string="Access Key", groups="backup_management.group_backup_admin"
    )
    secret_key_crypt = fields.Char(
        string="Encrypted Secret Key", groups="backup_management.group_backup_admin"
    )
    secret_key = fields.Char(
        string="Secret Key",
        compute="_compute_secret_key",
        inverse="_inverse_secret_key",
        groups="backup_management.group_backup_admin",
    )

    keep_daily = fields.Integer(string="Keep Daily", default=7)
    keep_weekly = fields.Integer(string="Keep Weekly", default=4)
    keep_monthly = fields.Integer(string="Keep Monthly", default=6)
    exclude_patterns = fields.Text(
        string="Exclude Patterns (.kopiaignore)", help="One pattern per line."
    )

    restore_drill_script = fields.Char(
        string="Automated Restore Drill Script",
        help="Absolute path to a shell script that performs a test restore and data validation.",
    )
    last_drill_time = fields.Datetime(string="Last Successful Drill", readonly=True)

    snapshot_ids = fields.One2many("backup.snapshot", "config_id", string="Snapshots")

    _name_uniq = models.Constraint(
        "UNIQUE(name)", "The backup configuration name must be unique!"
    )
    _target_uniq = models.Constraint(
        "UNIQUE(engine, target_path)", "The target path must be unique per engine!"
    )

    _name_not_empty = models.Constraint(
        "CHECK(LENGTH(TRIM(name)) > 0)", "The configuration name cannot be empty."
    )
    _target_path_not_empty = models.Constraint(
        "CHECK(LENGTH(TRIM(target_path)) > 0)", "The target path cannot be empty."
    )
    _retention_positive = models.Constraint(
        "CHECK(keep_daily >= 0 AND keep_weekly >= 0 AND keep_monthly >= 0)",
        "Retention values cannot be negative.",
    )
    _min_size_positive = models.Constraint(
        "CHECK(minimum_size_mb >= 0)", "Minimum size threshold cannot be negative."
    )

    def _get_fernet(self):
        key = os.environ.get("ODOO_BACKUP_CRYPTO_KEY") or os.environ.get("HAMS_CRYPTO_KEY")  # burn-ignore-env
        if not key:
            return None
        try:
            return Fernet(key.encode("utf-8"))
        except (ValueError, TypeError):
            return None

    def _crypt_field(self, value, decrypt=False):
        f = self._get_fernet()
        if not f or not value:
            return False
        try:
            if decrypt:
                return f.decrypt(value.encode("utf-8")).decode("utf-8")
            else:
                return f.encrypt(value.encode("utf-8")).decode("utf-8")
        except ValueError as e:
            logging.getLogger(__name__).warning("Encryption/Decryption error: %s", e)
            return "***ERROR***" if decrypt else False

    @api.depends("kopia_password_crypt")
    def _compute_kopia_password(self):
        for rec in self:
            rec.kopia_password = rec._crypt_field(
                rec.kopia_password_crypt, decrypt=True
            )

    def _inverse_kopia_password(self):
        for rec in self:
            rec.kopia_password_crypt = rec._crypt_field(rec.kopia_password)

    @api.depends("secret_key_crypt")
    def _compute_secret_key(self):
        for rec in self:
            rec.secret_key = rec._crypt_field(rec.secret_key_crypt, decrypt=True)

    def _inverse_secret_key(self):
        for rec in self:
            rec.secret_key_crypt = rec._crypt_field(rec.secret_key)

    @api.constrains("target_path", "restore_drill_script", "engine", "storage_type")
    def _check_security_paths(self):
        for rec in self:
            if rec.engine == "kopia" and rec.storage_type == "local":
                validate_backup_path(rec.target_path)

            if rec.engine == "pgbackrest":
                # pgBackRest target_path is a stanza name, not a direct path.
                # Ensure no shell metacharacters in stanza name.
                if (
                    not rec.target_path
                    or ";" in rec.target_path
                    or "&" in rec.target_path
                    or "|" in rec.target_path
                ):
                    raise UserError(
                        _("Invalid pgBackRest stanza name: %s") % rec.target_path
                    )

            if rec.restore_drill_script:
                validate_backup_path(rec.restore_drill_script)

    def _get_executable(self, engine):
        """
        Retrieves the path to the backup engine binary.
        If Kopia is missing, it attempts to download it via binary_downloader.
        """
        cmd_path = shutil.which(engine)
        if cmd_path:
            return cmd_path

        if engine == "pgbackrest":
            raise UserError(
                _(
                    "pgBackRest is missing. It requires OS-level PostgreSQL dependencies and must be installed via your package manager (e.g., 'sudo apt-get install pgbackrest')."
                )
            )

        if engine == "kopia":
            msg_body = _(
                "Kopia binary not found. Deferring to central generalized downloader..."
            )
            # Use Service ID for security & audit trails
            svc_uid = self.env["zero_sudo.security.utils"]._get_service_uid(
                "backup_management.user_backup_service_internal"
            )
            self.with_user(svc_uid).message_post(body=msg_body)  # audit-ignore-mail: Tested by [@ANCHOR: test_kopia_auto_download]  # fmt: skip

            # Binary manifestation is centralized in binary_downloader
            bin_path = (
                self.env["binary.manifest"]
                .with_user(svc_uid)
                .with_context(active_test=False)
                .ensure_executable("kopia")
            )

            msg_body = _("Kopia successfully installed to %s") % bin_path
            self.with_user(svc_uid).message_post(body=msg_body)  # audit-ignore-mail: Tested by [@ANCHOR: test_kopia_auto_download]  # fmt: skip
            return bin_path

        raise UserError(_("Unknown engine: %s") % engine)

    def _publish_to_worker(self, engine, payload_extra=None):
        """
        Internal helper to offload tasks to the RabbitMQ Bastion.
        """
        if not self.env.su and not self.env.user.has_group(
            "backup_management.group_backup_admin"
        ):
            raise AccessError(
                _("Only Backup Administrators can trigger backup operations.")
            )

        jobs = self.env["backup.job"]
        created_jobs = []

        for rec in self:
            job = jobs.create(
                {
                    "config_id": rec.id,
                    "website_id": rec.website_id.id,
                    "job_type": (
                        rec.engine if engine != "restore_cmd" else "kopia"
                    ),  # map restore_cmd to engine for UI
                    "state": "pending",
                    "output_log": "Queued in RabbitMQ...",
                }
            )
            created_jobs.append(job)

            payload_dict = {
                "job_id": job.id,
                "config_id": rec.id,
                "engine": engine,
                "target_path": rec.target_path,
                "svc_uid": self.env.uid,
                "website_id": rec.website_id.id,
            }
            if payload_extra:
                payload_dict.update(payload_extra)

            payload = json.dumps(payload_dict)

            def publish_task(msg=payload):
                if odoo.tools.config.get("test_enable"):
                    return
                try:
                    rmq_host = os.environ.get("RMQ_HOST") or "rabbitmq"
                    rmq_user = os.environ.get("RMQ_USER") or "guest"
                    rmq_pass = os.environ.get("RMQ_PASS") or "guest"  # burn-ignore-env
                    credentials = pika.PlainCredentials(rmq_user, rmq_pass)
                    conn_params = pika.ConnectionParameters(
                        host=rmq_host, credentials=credentials
                    )
                    connection = pika.BlockingConnection(conn_params)
                    channel = connection.channel()
                    channel.queue_declare(queue="backup_tasks", durable=True)
                    channel.basic_publish(
                        exchange="",
                        routing_key="backup_tasks",
                        body=msg,
                        properties=pika.BasicProperties(delivery_mode=2),
                    )
                    connection.close()
                except pika.exceptions.AMQPError as e:
                    logging.getLogger(__name__).warning("An error occurred: %s", e)
                    logging.getLogger(__name__).error(
                        "Failed to publish backup task to RMQ: %s", e
                    )

            self.env.cr.postcommit.add(publish_task)

        if len(created_jobs) == 1:
            return {
                "type": "ir.actions.act_window",
                "res_model": "backup.job",
                "res_id": created_jobs[0].id,
                "view_mode": "form",
                "target": "current",
            }
        return True

    def action_trigger_backup(self):
        # [@ANCHOR: backup_management:backup_trigger_execution]
        # Verified by [@ANCHOR: backup_management:test_backup_orchestration]
        # Implements ADR-0071: Asynchronous Bastion Pattern
        # Use Service ID for security & audit trails
        svc_uid = self.env["zero_sudo.security.utils"]._get_service_uid(
            "backup_management.user_backup_service_internal"
        )
        res = True
        for rec in self:
            if rec.engine == "kopia" and rec.storage_type == "local":
                validate_backup_path(rec.target_path)
            res = rec.with_user(svc_uid)._publish_to_worker(rec.engine)
        return res

    def action_apply_policies(self):
        # [@ANCHOR: backup_management:backup_apply_policies]
        # Verified by [@ANCHOR: test_apply_policies]
        # Use Service ID for security & audit trails
        svc_uid = self.env["zero_sudo.security.utils"]._get_service_uid(
            "backup_management.user_backup_service_internal"
        )
        for rec in self:
            if rec.engine == "kopia" and rec.storage_type == "local":
                validate_backup_path(rec.target_path)
            rec.with_user(svc_uid)._publish_to_worker("kopia_policy")
        return True

    def action_sync_snapshots(self):
        # [@ANCHOR: UX_BACKUP_SYNC]
        # [@ANCHOR: backup_management:backup_sync_kopia]
        # [@ANCHOR: backup_management:backup_sync_pgbackrest]
        # Verified by [@ANCHOR: backup_management:test_backup_cron]
        # Use Service ID for security & audit trails
        svc_uid = self.env["zero_sudo.security.utils"]._get_service_uid(
            "backup_management.user_backup_service_internal"
        )
        for rec in self:
            if rec.engine == "kopia" and rec.storage_type == "local":
                validate_backup_path(rec.target_path)
            rec.with_user(svc_uid)._publish_to_worker("sync_snapshots")
        return True

    def _execute_restore_drill(self):
        """
        Offloads the restore drill to the worker.
        """
        if self.restore_drill_script:
            validate_backup_path(self.restore_drill_script)
        # Use Service ID for security & audit trails
        svc_uid = self.env["zero_sudo.security.utils"]._get_service_uid(
            "backup_management.user_backup_service_internal"
        )
        return self.with_user(svc_uid)._publish_to_worker(
            "restore_drill", {"script": self.restore_drill_script}
        )

    def _report_backup_failure(self, message):
        # [@ANCHOR: backup_management:backup_pager_synergy]
        # Verified by [@ANCHOR: backup_management:test_backup_cron]
        # Verified by [@ANCHOR: test_trigger_kopia_and_pgbackrest]
        if "pager.incident" in self.env:
            try:
                pager_uid = self.env["zero_sudo.security.utils"]._get_service_uid(
                    "pager_duty.user_pager_service_internal"
                )
                self.env["pager.incident"].with_user(pager_uid).report_incident(
                    {
                        "source": f"Backup Manager: {self.name}",
                        "severity": "critical",
                        "description": message,
                    }
                )
            except (UserError, AccessError, ValueError) as e:
                logging.getLogger(__name__).warning("An error occurred: %s", e)

        svc_uid = self.env["zero_sudo.security.utils"]._get_service_uid(
            "backup_management.user_backup_service_internal"
        )
        self.with_user(svc_uid).message_post(
            body=message
        )  # audit-ignore-mail: Tested by [@ANCHOR: backup_management:backup_pager_synergy]  # fmt: skip

    @api.model
    def get_board_data(self):
        # [@ANCHOR: backup_management:backup_board_data]
        # Verified by [@ANCHOR: backup_management:test_backup_view]
        domain = []
        if self.env.context.get("website_id"):
            domain = [
                "|",
                ("website_id", "=", False),
                ("website_id", "=", self.env.context.get("website_id")),
            ]
        configs = self.search_read(domain, ["name", "engine", "target_path"])
        now = fields.Datetime.now()

        latest_snaps = self.env["backup.latest.snapshot.view"].search_read(
            [], ["config_id", "snapshot_id", "start_time", "size_bytes", "status"]
        )
        snap_map = {s["config_id"][0]: s for s in latest_snaps if s.get("config_id")}

        for c in configs:
            snap = snap_map.get(c["id"])
            if snap:
                c["latest_snapshot"] = snap
                if snap.get("start_time"):
                    # Handle string or datetime
                    start_time = snap["start_time"]
                    if isinstance(start_time, str):
                        start_time = fields.Datetime.to_datetime(start_time)
                    delta = (now - start_time).total_seconds()
                else:
                    delta = 999999
                c["is_stale"] = delta > (26 * 60 * 60)
            else:
                c["latest_snapshot"] = False
                c["is_stale"] = True
        return configs

    def _process_snapshot_data(self, data, engine):
        Snapshot = self.env["backup.snapshot"]
        existing_snaps = Snapshot.search([("config_id", "=", self.id)], limit=5000)
        existing_map = {s.snapshot_id: s for s in existing_snaps}

        creates = []
        if engine == "kopia":
            for snap in data:
                sid = snap.get("id")
                if sid and sid not in existing_map:
                    creates.append(
                        {
                            "config_id": self.id,
                            "snapshot_id": sid,
                            "start_time": snap.get("startTime", "")[:19].replace(
                                "T", " "
                            ),
                            "size_bytes": snap.get("summary", {}).get("totalBytes", 0),
                            "status": "completed",
                        }
                    )
        elif engine == "pgbackrest":
            for stanza in data:
                for snap in stanza.get("backup", []):
                    sid = snap.get("label")
                    if sid and sid not in existing_map:
                        ts = snap.get("timestamp", {}).get("start", 0)
                        dt = (
                            datetime.datetime.utcfromtimestamp(ts).strftime(
                                "%Y-%m-%d %H:%M:%S"
                            )
                            if ts
                            else False
                        )
                        creates.append(
                            {
                                "config_id": self.id,
                                "snapshot_id": sid,
                                "start_time": dt,
                                "size_bytes": snap.get("info", {}).get("size", 0),
                                "status": "completed",
                            }
                        )

        if creates:
            # Sort by start_time to ensure we check the absolute latest if multiple are created
            creates.sort(key=lambda x: x["start_time"], reverse=True)
            Snapshot.create(creates)
            if self.minimum_size_mb > 0:
                for c in creates:
                    snap_mb = c.get("size_bytes", 0) / (1024 * 1024)
                    if snap_mb < self.minimum_size_mb:
                        self._report_backup_failure(
                            f"Snapshot Anomaly: Snapshot {c.get('snapshot_id')} is {snap_mb:.2f} MB, below the {self.minimum_size_mb} MB minimum."
                        )

    @api.model
    def cron_sync_all_backups(self):
        # [@ANCHOR: backup_management:cron_sync_all_backups]
        # Verified by [@ANCHOR: backup_management:test_backup_cron]
        # Use Service ID for security & audit trails
        svc_uid = self.env["zero_sudo.security.utils"]._get_service_uid(
            "backup_management.user_backup_service_internal"
        )
        configs = self.env["backup.config"].with_user(svc_uid).search([], limit=1000)
        now = fields.Datetime.now()
        for conf in configs:
            conf.action_sync_snapshots()

            snaps = conf.snapshot_ids.sorted(lambda s: s.start_time, reverse=True)
            latest_snap = snaps[0] if snaps else None
            if latest_snap and latest_snap.start_time:
                delta = (now - latest_snap.start_time).total_seconds()
                if delta > (
                    26 * 60 * 60
                ):  # 26 hours (allows for 24h cron jitter without false positives)
                    conf._report_backup_failure(
                        f"Stale Backup Alert: No new snapshots detected for {conf.name} in over 26 hours."
                    )

            if conf.restore_drill_script:
                delta_drill = (
                    (now - conf.last_drill_time).total_seconds()
                    if conf.last_drill_time
                    else 9999999
                )
                if delta_drill > (7 * 24 * 60 * 60):  # 7 Days
                    conf._execute_restore_drill()
