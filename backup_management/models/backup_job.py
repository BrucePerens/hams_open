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

    # How long a job may sit in "pending" before it is written off as never picked up. Jobs are
    # hourly and one runs at a time (prefetch 1); the longest real job, a full pgBackRest backup,
    # takes about 40 minutes, so a healthy job leaves "pending" within the hour.
    PENDING_TIMEOUT_HOURS = 6

    def _auto_refresh_status(self):
        """
        Cleanup abandoned jobs: those stuck in 'processing' for too long, and those that never
        left 'pending'. This ensures the UI doesn't show them as active forever if a worker dies
        or a queue message is lost.
        """
        # [@ANCHOR: backup_management:COMM_auto_refresh_status]
        self._fail_stale_pending_jobs()
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

    # [@ANCHOR: backup_management:COMM_fail_stale_pending_jobs]
    def _fail_stale_pending_jobs(self):
        """
        Mark jobs still 'pending' after PENDING_TIMEOUT_HOURS as failed, with the reason in the log.

        Found on hams1 2026-10-12: eight jobs (ids 5, 6, 143, 218-222) read "Queued in RabbitMQ..."
        for a week. 218-222 were consumed while the worker's Odoo API key was rejected (HTTP 401),
        so the worker could neither mark them "processing" nor report the failure, and acked them;
        the others' messages were gone from the queue. Nothing ever moved them out of 'pending',
        and the worker only ever updates a job it receives. This is the Odoo-side backstop.
        A job a worker did pick up is 'processing' and is not touched here.
        """
        limit = fields.Datetime.now() - datetime.timedelta(hours=self.PENDING_TIMEOUT_HOURS)
        stale = self.env["backup.job"].search(
            [("state", "=", "pending"), ("create_date", "<", limit)], limit=1000
        )
        for job in stale:
            _logger.warning(
                "Backup job %s (config %s) was still pending after %s hours; marking it failed.",
                job.id, job.config_id.id, self.PENDING_TIMEOUT_HOURS,
            )
            job.write(
                {
                    "state": "failed",
                    "output_log": (job.output_log or "")
                    + "\n[SYSTEM] No backup worker picked this job up within "
                    f"{self.PENDING_TIMEOUT_HOURS} hours (the queue message was lost, or the worker "
                    "could not reach Odoo). It was NOT run. Trigger it again.",
                }
            )
        return len(stale)

    # [@ANCHOR: backup_management:COMM_action_refresh_status]
    def action_refresh_status(self):
        """
        Manually trigger a status refresh.
        In this implementation, it's a no-op that just returns True to allow the UI to refresh.
        """
        return True
