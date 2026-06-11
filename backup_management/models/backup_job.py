# -*- coding: utf-8 -*-
import datetime
from odoo import models, fields


class BackupJob(models.Model):
    _name = "backup.job"
    _description = "Asynchronous Backup Job"
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

    def _auto_refresh_status(self):
        """
        Cleanup abandoned jobs that have been stuck in 'processing' for too long.
        This ensures the UI doesn't show them as active forever if a worker dies.
        """
        # [@ANCHOR: backup_management:auto_refresh_status]
        timeout_limit = fields.Datetime.now() - datetime.timedelta(hours=2)
        abandoned_jobs = self.env["backup.job"].search(
            [
                ("state", "=", "processing"),
                ("write_date", "<", timeout_limit),
            ],
            limit=1,
        )
        if abandoned_jobs:
            abandoned_jobs.write(
                {
                    "state": "failed",
                    "output_log": (abandoned_jobs[0].output_log or "")
                    + "\n[SYSTEM] Job timed out after 2 hours of inactivity.",
                }
            )
            # Re-trigger to process the next one in the next cron run or via _trigger if available
            self.env.ref("backup_management.ir_cron_auto_refresh_backup_jobs")._trigger()

    def action_refresh_status(self):
        """
        Manually trigger a status refresh.
        In this implementation, it's a no-op that just returns True to allow the UI to refresh.
        """
        return True
