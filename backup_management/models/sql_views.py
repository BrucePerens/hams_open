# -*- coding: utf-8 -*-
from odoo import models, fields, tools


class BackupLatestSnapshotView(models.Model):
    _name = "backup.latest.snapshot.view"
    _description = "Latest Backup Snapshot View"
    _auto = False

    config_id = fields.Many2one("backup.config", readonly=True)
    website_id = fields.Many2one("website", readonly=True)
    company_id = fields.Many2one("res.company", readonly=True)
    snapshot_id = fields.Char(readonly=True)
    start_time = fields.Datetime(readonly=True)
    size_bytes = fields.Float(readonly=True)
    status = fields.Char(readonly=True)

    def init(self):
        tools.drop_view_if_exists(self.env.cr, self._table)
        self.env.cr.execute("""
            CREATE OR REPLACE VIEW backup_latest_snapshot_view AS (
                SELECT DISTINCT ON (config_id)
                    id, config_id, website_id, company_id, snapshot_id, start_time, size_bytes, status
                FROM backup_snapshot
                ORDER BY config_id, start_time DESC, id DESC
            )
        """)
