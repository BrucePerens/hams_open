# -*- coding: utf-8 -*-
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
