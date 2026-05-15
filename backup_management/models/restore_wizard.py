# -*- coding: utf-8 -*-
import json
import os
import pika
import logging
from odoo import models, fields
from .utils import validate_backup_path

class BackupRestoreWizard(models.TransientModel):
    _name = "backup.restore.wizard"
    _description = "Backup Restore Wizard"

    snapshot_id = fields.Many2one("backup.snapshot", string="Snapshot", required=True, readonly=True)
    restore_target_path = fields.Char(string="Restore Directory / Stanza Target", required=True, help="Path where the backup should be restored, or stanza to target.")

    def action_restore(self):
        if self.snapshot_id.config_id.engine == "kopia":
            validate_backup_path(self.restore_target_path)

        jobs = self.env["backup.job"]
        job = jobs.create({
            "config_id": self.snapshot_id.config_id.id,
            "job_type": self.snapshot_id.config_id.engine,
            "state": "pending",
            "output_log": "Restore queued in RabbitMQ...",
        })

        cmd_args = []
        if self.snapshot_id.config_id.engine == "kopia":
            cmd_args = ["kopia", "restore", self.snapshot_id.snapshot_id, self.restore_target_path]
        elif self.snapshot_id.config_id.engine == "pgbackrest":
            cmd_args = ["pgbackrest", "restore", f"--stanza={self.restore_target_path}", f"--set={self.snapshot_id.snapshot_id}"]

        payload = json.dumps({
            "job_id": job.id,
            "config_id": self.snapshot_id.config_id.id,
            "engine": "restore_cmd",
            "cmd_args": cmd_args,
            "snapshot_id": self.snapshot_id.snapshot_id,
        })

        def publish_task(msg=payload):
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

        self.env.cr.postcommit.add(publish_task)

        return {
            "type": "ir.actions.act_window",
            "res_model": "backup.job",
            "res_id": job.id,
            "view_mode": "form",
            "target": "current",
        }
