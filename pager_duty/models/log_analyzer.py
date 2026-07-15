# This software is distributed under the terms of the Affero General Public License (AGPL-3).
# SPDX-License-Identifier: AGPL-3.0-or-later

# -*- coding: utf-8 -*-
from odoo import models, fields, api


class PagerLogPattern(models.Model):
    """
    Regex pattern for log analysis.
    This model is multi-tenant and multi-website, partitioned by website_id.
    """

    _name = "pager.log.pattern"
    _description = "Log Analyzer Regex Pattern"
    _order = "severity desc, name asc"

    name = fields.Char(
        string="Pattern Name", required=True, help="e.g. Kernel Filesystem Corruption"
    )
    website_id = fields.Many2one("website", string="Website", ondelete="cascade")
    regex = fields.Char(
        string="Regular Expression", required=True, help="e.g. (ext4|xfs|btrfs).*error"
    )
    severity = fields.Selection(
        [
            ("low", "Low"),
            ("medium", "Medium"),
            ("high", "High"),
            ("critical", "Critical"),
        ],
        string="Severity",
        default="high",
        required=True,
    )
    active = fields.Boolean(default=True)

    _name_uniq = models.Constraint("UNIQUE(name, website_id)", "The pattern name must be unique per website!")


class PagerLogFile(models.Model):
    """
    Log file to be analyzed by the daemon.
    This model is multi-tenant and multi-website, partitioned by website_id.
    """

    _name = "pager.log.file"
    _description = "Log Analyzer Target File"
    name = fields.Char(string="Name", default=lambda self: self._description)

    filepath = fields.Char(
        string="Absolute Path", required=True, help="e.g. /var/log/syslog"
    )
    website_id = fields.Many2one("website", string="Website", ondelete="cascade")
    active = fields.Boolean(default=True)

    _path_uniq = models.Constraint("UNIQUE(filepath, website_id)", "The file path must be unique per website!")

class PagerLogSearchJob(models.TransientModel):
    _name = "pager.log.search.job"
    _description = "Log Search Job State"

    name = fields.Char(string="Name", default="Log Search Job")
    uuid = fields.Char(string="UUID", required=True, index=True)
    state = fields.Selection([("pending", "Pending"), ("done", "Done"), ("error", "Error")], default="pending")
    result_payload = fields.Text(string="Result JSON")

    @api.model
    def rpc_update_state(self, uuid, state, result_payload):
        job = self.env["pager.log.search.job"].search([("uuid", "=", uuid)], limit=1)
        if job:
            job.write({"state": state, "result_payload": result_payload})
            return True
        return False
