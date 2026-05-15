# -*- coding: utf-8 -*-
from odoo import models, fields, api


class BackupSnapshot(models.Model):
    _name = "backup.snapshot"
    _description = "Backup Snapshot"
    _order = "start_time desc"

    config_id = fields.Many2one(
        "backup.config", string="Configuration", required=True, ondelete="cascade"
    )
    snapshot_id = fields.Char(string="Snapshot ID / Label", required=True)
    start_time = fields.Datetime(string="Start Time")
    size_bytes = fields.Integer(string="Size (Bytes)")
    status = fields.Char(string="Status")

    restore_command = fields.Char(
        string="Restore Command", compute="_compute_restore_command"
    )

    _snapshot_uniq = models.Constraint(
        "UNIQUE(config_id, snapshot_id)",
        "Snapshot IDs must be unique per configuration!",
    )

    @api.depends("snapshot_id", "config_id.engine", "config_id.target_path")
    def _compute_restore_command(self):
        # [@ANCHOR: backup_restore_command]
        # Verified by [@ANCHOR: test_restore_command_computation]
        for rec in self:
            if rec.config_id.engine == "kopia":
                rec.restore_command = (
                    f"kopia restore {rec.snapshot_id} /var/lib/odoo/backups/restore_{rec.snapshot_id}"
                )
            elif rec.config_id.engine == "pgbackrest":
                rec.restore_command = f"pgbackrest restore --stanza={rec.config_id.target_path} --set={rec.snapshot_id}"
            else:
                rec.restore_command = ""
