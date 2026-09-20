# -*- coding: utf-8 -*-
# Copyright © Bruce Perens K6BP. All Rights Reserved.
# SPDX-License-Identifier: AGPL-3.0-or-later
import datetime
import logging
from odoo import models, fields

_logger = logging.getLogger(__name__)


class BackupJob(models.Model):
    _name = "backup.job"
    _description = "Asynchronous Backup Job"
    name = fields.Char(string="Name", default=lambda self: self._description)
    _order = "create_date desc"

    config_id = fields.Many2one(
        "backup.config", string="Configuration", required=True, ondelete="cascade"
    )
    website_id = fields.Many2one(
        "website", string="Website", related="config_id.website_id", store=True
    )
    company_id = fields.Many2one(
        "res.company", string="Company", related="config_id.company_id", store=True
    )
    job_type = fields.Selection(
        [("kopia", "Kopia"), ("pgbackrest", "pgBackRest")],
        string="Job Type",
        required=True,
    )
    state = fields.Selection(
        [
            ("pending", "Pending"),
            ("processing", "Processing"),
            ("done", "Done"),
            ("failed", "Failed"),
        ],
        string="State",
        default="pending",
        required=True,
    )
    output_log = fields.Text(string="Live Output Log")

    # [@ANCHOR: backup_management:COMM_append_log]
    def append_log(self, text_chunk):
        """Append text delta to the output_log to prevent resending large buffers."""
        for record in self:
            if record.output_log:
                record.output_log += text_chunk
            else:
                record.output_log = text_chunk


    # [@ANCHOR: backup_management:COMM_job_dispatch_failed]
    def _mark_dispatch_failed(self):
        """The RabbitMQ send for this job failed after the commit (see
        utils.publish_to_rabbitmq). Only a job still "pending" is touched: a
        worker that already picked it up owns the state from then on."""
        for job in self:
            if job.state != "pending":
                continue
            _logger.error(
                "Backup job %s (config %s) could not be handed to RabbitMQ; marking it failed.",
                job.id, job.config_id.id,
            )
            job.write(
                {
                    "state": "failed",
                    "output_log": (job.output_log or "")
                    + "\n[SYSTEM] Could not hand this job to the backup worker queue "
                    "(RabbitMQ unavailable). It was NOT run. Trigger it again once the queue is back.",
                }
            )

    def _auto_refresh_status(self):
        """
        Cleanup abandoned jobs that have been stuck in 'processing' for too long.
        This ensures the UI doesn't show them as active forever if a worker dies.
        """
        # [@ANCHOR: backup_management:COMM_auto_refresh_status]
        timeout_limit = fields.Datetime.now() - datetime.timedelta(hours=2)
        abandoned_jobs = self.env["backup.job"].search(
            [
                ("state", "=", "processing"),
                ("write_date", "<", timeout_limit),
            ],
            limit=1000
        )
        for job in abandoned_jobs:
            timeout_msg = """\n[SYSTEM] Job timed out after 2 hours of inactivity."""
            job.write(
                {
                    "state": "failed",
                    "output_log": (job.output_log or "") + timeout_msg,
                }
            )

    # [@ANCHOR: backup_management:COMM_action_refresh_status]
    def action_refresh_status(self):
        """
        Manually trigger a status refresh.
        In this implementation, it's a no-op that just returns True to allow the UI to refresh.
        """
        return True
