# -*- coding: utf-8 -*-
# Copyright © Bruce Perens K6BP. All Rights Reserved.
# SPDX-License-Identifier: AGPL-3.0-or-later
from odoo import models, fields, api


# [@ANCHOR: backup_management_backup_snapshot_backup_snapshot]
class BackupSnapshot(models.Model):
    _name = "backup.snapshot"
    _description = "Backup Snapshot"
    name = fields.Char(string="Name", default=lambda self: self._description)
    _order = "start_time desc"

    config_id = fields.Many2one(
        "backup.config",
        string="Configuration",
        # History is kept when the configuration is deleted (Bruce, 2026-11-16); website_id and
        # company_id are stored copies, so the record stays scoped to its tenant.
        ondelete="set null",
        index=True,
    )
    # Stored copies of the configuration's website and company that a configuration delete does not clear
    # (a stored related field would be), so the kept record stays in its tenant.
    website_id = fields.Many2one("website", string="Website", compute="_compute_tenant", store=True)
    company_id = fields.Many2one("res.company", string="Company", compute="_compute_tenant", store=True)
    # [@ANCHOR: backup_management:snapshot_compute_tenant]
    # Verified by [@ANCHOR: backup_management:test_history_survives_config_delete]
    @api.depends("config_id.website_id", "config_id.company_id")
    def _compute_tenant(self):
        for rec in self:
            if rec.config_id:
                rec.website_id = rec.config_id.website_id
                rec.company_id = rec.config_id.company_id

    snapshot_id = fields.Char(string="Snapshot ID / Label", required=True)
    start_time = fields.Datetime(string="Start Time")
    size_bytes = fields.Float(string="Size (Bytes)")
    status = fields.Char(string="Status")

    restore_command = fields.Char(
        string="Restore Command", compute="_compute_restore_command"
    )

    _snapshot_uniq_msg = """Snapshot IDs must be unique per configuration!"""
    _snapshot_uniq = models.Constraint("UNIQUE(config_id, snapshot_id)", _snapshot_uniq_msg)

    @api.depends("snapshot_id", "config_id.engine", "config_id.target_path", "config_id.pgbackrest_repo")
    def _compute_restore_command(self):
        # [@ANCHOR: backup_management:COMM_backup_restore_command]

        # # Verified by [@ANCHOR: test_restore_command_computation]
        for rec in self:
            if not rec.snapshot_id or not rec.config_id:
                rec.restore_command = ""
                continue
            if rec.config_id.engine == "kopia":
                rec.restore_command = f"kopia restore {rec.snapshot_id} /var/lib/odoo/backups/restore_{rec.snapshot_id}"
            elif rec.config_id.engine == "pgbackrest":
                repo_option = (
                    f" --repo={rec.config_id.pgbackrest_repo}" if rec.config_id.pgbackrest_repo else ""
                )
                rec.restore_command = (
                    f"pgbackrest restore --stanza={rec.config_id.target_path} --set={rec.snapshot_id}{repo_option}"
                )
            else:
                rec.restore_command = ""
