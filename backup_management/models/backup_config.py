# -*- coding: utf-8 -*-
import logging
import json
import os
import datetime
import shutil
import pika
import odoo
from odoo import models, fields, api, _
from odoo.exceptions import UserError
from .utils import validate_backup_path

from cryptography.fernet import Fernet


class BackupConfig(models.Model):
    _name = "backup.config"
    _description = "Backup Configuration"
    _inherit = ["mail.thread"]

    name = fields.Char(string="Name", required=True)
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

    kopia_password_crypt = fields.Char(string="Encrypted Kopia Password")
    kopia_password = fields.Char(
        string="Kopia Password",
        compute="_compute_kopia_password",
        inverse="_inverse_kopia_password",
    )

    storage_type = fields.Selection(
        [("local", "Local Directory"), ("s3", "AWS S3"), ("b2", "Backblaze B2")],
        default="local",
        string="Storage Type",
    )
    bucket_name = fields.Char(string="Bucket Name")
    endpoint_url = fields.Char(string="Endpoint URL")
    access_key = fields.Char(string="Access Key")
    secret_key_crypt = fields.Char(string="Encrypted Secret Key")
    secret_key = fields.Char(
        string="Secret Key",
        compute="_compute_secret_key",
        inverse="_inverse_secret_key",
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
        key = os.environ.get("ODOO_BACKUP_CRYPTO_KEY") or os.environ.get("HAMS_CRYPTO_KEY")
        if key and Fernet:
            return Fernet(key.encode("utf-8"))
        return None

    @api.depends("kopia_password_crypt")
    def _compute_kopia_password(self):
        f = self._get_fernet()
        for rec in self:
            if rec.kopia_password_crypt and f:
                try:
                    rec.kopia_password = f.decrypt(
                        rec.kopia_password_crypt.encode("utf-8")
                    ).decode("utf-8")
                except Exception as e:
                    logging.getLogger(__name__).warning("An error occurred: %s", e)
            else:
                rec.kopia_password = False

    def _inverse_kopia_password(self):
        f = self._get_fernet()
        for rec in self:
            if rec.kopia_password and f:
                rec.kopia_password_crypt = f.encrypt(
                    rec.kopia_password.encode("utf-8")
                ).decode("utf-8")
            else:
                rec.kopia_password_crypt = False

    @api.depends("secret_key_crypt")
    def _compute_secret_key(self):
        f = self._get_fernet()
        for rec in self:
            if rec.secret_key_crypt and f:
                try:
                    rec.secret_key = f.decrypt(
                        rec.secret_key_crypt.encode("utf-8")
                    ).decode("utf-8")
                except Exception as e:
                    logging.getLogger(__name__).warning("An error occurred: %s", e)
                    rec.secret_key = "***DECRYPT_FAILED***"
            else:
                rec.secret_key = False

    def _inverse_secret_key(self):
        f = self._get_fernet()
        for rec in self:
            if rec.secret_key and f:
                rec.secret_key_crypt = f.encrypt(rec.secret_key.encode("utf-8")).decode(
                    "utf-8"
                )
            else:
                rec.secret_key_crypt = False

    @api.constrains("target_path", "restore_drill_script", "engine", "storage_type")
    def _check_security_paths(self):
        for rec in self:
            if rec.engine == "kopia" and rec.storage_type == "local":
                validate_backup_path(rec.target_path)
            # pgBackRest target_path is a stanza name, not a direct path,
            # but we still validate the restore drill script path.
            if rec.restore_drill_script:
                validate_backup_path(rec.restore_drill_script)

    def _get_executable(self, engine):
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
            mail_svc = self.env["zero_sudo.security.utils"]._get_service_uid(
                "zero_sudo.mail_service_internal"
            )
            self.with_user(mail_svc).message_post(body=msg_body)  # audit-ignore-mail: Tested by [@ANCHOR: test_kopia_auto_download]  # fmt: skip
            # Ensure the installation happens under the context of the service user to maintain proper ACLs
            bin_path = (
                self.env["binary.manifest"]
                .with_context(active_test=False)
                .ensure_executable("kopia")
            )
            msg_body = _("Kopia successfully installed to %s") % bin_path
            self.with_user(mail_svc).message_post(body=msg_body)  # audit-ignore-mail: Tested by [@ANCHOR: test_kopia_auto_download]  # fmt: skip
            return bin_path

        raise UserError(_("Unknown engine: %s") % engine)

    def _publish_to_worker(self, engine, payload_extra=None):
        """
        Internal helper to offload tasks to the RabbitMQ Bastion.
        """
        jobs = self.env["backup.job"]
        created_jobs = []

        for rec in self:
            job = jobs.create(
                {
                    "config_id": rec.id,
                    "job_type": rec.engine if engine != "restore_cmd" else "kopia",  # map restore_cmd to engine for UI
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
                    rmq_pass = os.environ.get("RMQ_PASS") or "guest"
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
                except Exception as e:
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
        # [@ANCHOR: backup_trigger_execution]
        # Verified by [@ANCHOR: test_backup_orchestration]
        # Implements ADR-0071: Asynchronous Bastion Pattern
        return self._publish_to_worker(self.engine)

    def action_apply_policies(self):
        # [@ANCHOR: backup_apply_policies]
        # Verified by [@ANCHOR: test_apply_policies]
        return self._publish_to_worker("kopia_policy")

    def action_sync_snapshots(self):
        # [@ANCHOR: UX_BACKUP_SYNC]
        # [@ANCHOR: backup_sync_kopia]
        # [@ANCHOR: backup_sync_pgbackrest]
        # Verified by [@ANCHOR: test_backup_cron]
        return self._publish_to_worker("sync_snapshots")

    def _execute_restore_drill(self):
        """
        Offloads the restore drill to the worker.
        """
        return self._publish_to_worker("restore_drill", {"script": self.restore_drill_script})

    def _report_backup_failure(self, message):
        # [@ANCHOR: backup_pager_synergy]
        # Verified by [@ANCHOR: test_backup_cron]
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
            except Exception as e:
                logging.getLogger(__name__).warning("An error occurred: %s", e)

        mail_svc = self.env["zero_sudo.security.utils"]._get_service_uid(
            "zero_sudo.mail_service_internal"
        )
        self.with_user(mail_svc).message_post(body=message) # audit-ignore-mail: Tested by [@ANCHOR: backup_pager_synergy]  # fmt: skip

    @api.model
    def get_board_data(self):
        # [@ANCHOR: backup_board_data]
        # Verified by [@ANCHOR: test_backup_view]
        configs = self.search_read([], ["name", "engine", "target_path"])
        now = fields.Datetime.now()

        latest_snaps = self.env["backup.latest.snapshot.view"].search_read(
            [], ["config_id", "snapshot_id", "start_time", "size_bytes", "status"]
        )
        snap_map = {s["config_id"][0]: s for s in latest_snaps if s.get("config_id")}

        for c in configs:
            snap = snap_map.get(c["id"])
            if snap:
                c["latest_snapshot"] = snap
                delta = (
                    (now - snap["start_time"]).total_seconds()
                    if snap["start_time"]
                    else 999999
                )
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
        # [@ANCHOR: cron_sync_all_backups]
        # Verified by [@ANCHOR: test_backup_cron]
        configs = self.env["backup.config"].search([], limit=1000)
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
