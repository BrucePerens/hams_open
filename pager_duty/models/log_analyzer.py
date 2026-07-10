# -*- coding: utf-8 -*-
from odoo import models, fields


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
