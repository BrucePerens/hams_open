# -*- coding: utf-8 -*-
import json
import os
import pika
import logging
from odoo import models, fields, _
from odoo.exceptions import UserError, AccessError
from .utils import validate_backup_path


class BackupRestoreWizard(models.TransientModel):
    _name = "backup.restore.wizard"
    _description = "Backup Restore Wizard"

    snapshot_id = fields.Many2one(
        "backup.snapshot", string="Snapshot", required=True, readonly=True
    )
    restore_target_path = fields.Char(
        string="Restore Directory / Stanza Target",
        required=True,
        help="Path where the backup should be restored, or stanza to target.",
    )

    def action_restore(self):
        # [@ANCHOR: backup_trigger_restore]
        # Verified by [@ANCHOR: test_restore_action]
        if not self.env.user.has_group("backup_management.group_backup_admin"):
            raise AccessError(
                _("Only Backup Administrators can trigger restore operations.")
            )

        if self.snapshot_id.config_id.engine == "kopia":
            validate_backup_path(self.restore_target_path)

        # Additional safety check for pgbackrest stanza
        if self.snapshot_id.config_id.engine == "pgbackrest":
            if not self.restore_target_path:
                raise UserError(_("Restore target stanza is required."))
            validate_backup_path(self.restore_target_path)

        # Use Service ID for security & audit trails
        svc_uid = self.env["zero_sudo.security.utils"]._get_service_uid(
            "backup_management.user_backup_service_internal"
        )
        jobs = self.env["backup.job"].with_user(svc_uid)
        job = jobs.create(
            {
                "config_id": self.snapshot_id.config_id.id,
                "website_id": self.snapshot_id.config_id.website_id.id,
                "job_type": self.snapshot_id.config_id.engine,
                "state": "pending",
                "output_log": "Restore queued in RabbitMQ...",
            }
        )

        cmd_args = []
        if self.snapshot_id.config_id.engine == "kopia":
            cmd_args = [
                "kopia",
                "restore",
                self.snapshot_id.snapshot_id,
                self.restore_target_path,
            ]
        elif self.snapshot_id.config_id.engine == "pgbackrest":
            # Using list for subprocess ensures no shell injection
            cmd_args = [
                "pgbackrest",
                "restore",
                f"--stanza={self.restore_target_path}",
                f"--set={self.snapshot_id.snapshot_id}",
            ]

        payload = json.dumps(
            {
                "job_id": job.id,
                "config_id": self.snapshot_id.config_id.id,
                "engine": "restore_cmd",
                "cmd_args": cmd_args,
                "snapshot_id": self.snapshot_id.snapshot_id,
                "svc_uid": svc_uid,  # Pass svc_uid for worker to potentially use
            }
        )

        def publish_task(msg=payload):
            try:
                utils = self.env["zero_sudo.security.utils"]
                rmq_host = (
                    utils._get_system_param("backup_management.rmq_host")
                    or os.environ.get("RMQ_HOST")
                    or "rabbitmq"
                )
                rmq_user = (
                    utils._get_system_param("backup_management.rmq_user")
                    or os.environ.get("RMQ_USER")
                    or "guest"
                )
                rmq_pass = (
                    utils._get_system_param("backup_management.rmq_pass")
                    or os.environ.get("RMQ_PASS")  # burn-ignore-env
                    or "guest"
                )  # burn-ignore-env
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

        self.env.cr.postcommit.add(publish_task)

        return {
            "type": "ir.actions.act_window",
            "res_model": "backup.job",
            "res_id": job.id,
            "view_mode": "form",
            "target": "current",
        }
