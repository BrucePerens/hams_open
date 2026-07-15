# -*- coding: utf-8 -*-
# Copyright © Bruce Perens K6BP. All Rights Reserved.
# This software is released under the AGPL-3.0 License.
from odoo import models, fields, tools


class BackupLatestSnapshotView(models.Model):
    _name = "backup.latest.snapshot.view"
    _description = "Latest Backup Snapshot View"
    name = fields.Char(string="Name", default=lambda self: self._description)
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
        with self.env.cr.savepoint():
            self.env.cr.execute(
                """
                CREATE OR REPLACE VIEW backup_latest_snapshot_view AS (
                    SELECT DISTINCT ON (config_id) 'SQL View'::varchar as name,
                        id, config_id, website_id, company_id, snapshot_id, start_time, size_bytes, status
                    FROM backup_snapshot
                    ORDER BY config_id, start_time DESC, id DESC
                )
            """
            )

        # Performance: Postgres procedure to upsert snapshots in a single round-trip
        # [@ANCHOR: backup_management:COMM_upsert_snapshots_procedure]
        with self.env.cr.savepoint():
            self.env.cr.execute(
                """
            CREATE OR REPLACE FUNCTION upsert_backup_snapshots(
                p_config_id INTEGER,
                p_website_id INTEGER,
                p_company_id INTEGER,
                p_snapshots JSONB,
                p_user_id INTEGER
            ) RETURNS TABLE(out_snapshot_id TEXT) AS $$
            BEGIN
                RETURN QUERY
                INSERT INTO backup_snapshot (
                    config_id, website_id, company_id, snapshot_id, start_time, size_bytes, status,
                    create_uid, create_date, write_uid, write_date
                )
                SELECT
                    p_config_id,
                    p_website_id,
                    p_company_id,
                    s->>'snapshot_id',
                    (s->>'start_time')::timestamp,
                    (s->>'size_bytes')::float,
                    s->>'status',
                    p_user_id, NOW(), p_user_id, NOW()
                FROM jsonb_array_elements(p_snapshots) s
                ON CONFLICT (config_id, snapshot_id) DO NOTHING
                RETURNING backup_snapshot.snapshot_id;
            END;
            $$ LANGUAGE plpgsql;
        """
        )
