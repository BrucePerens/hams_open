# -*- coding: utf-8 -*-
# Copyright © Bruce Perens K6BP. All Rights Reserved.
# SPDX-License-Identifier: AGPL-3.0-or-later
"""The board's last snapshot and stale flag ignore partial snapshots, as the stale-backup alert does."""
import datetime

from odoo import fields
from odoo.tests.common import tagged

from odoo.addons.zero_sudo.tests.common import HamsTransactionCase


@tagged("post_install", "-at_install")
class TestBoardLatestPartial(HamsTransactionCase):
    def _board_row(self, config):
        return next(r for r in self.env["backup.config"].get_board_data() if r["id"] == config.id)

    def _snap(self, config, sid, hours_ago, status):
        return self.env["backup.snapshot"].create(
            {
                "config_id": config.id,
                "snapshot_id": sid,
                "start_time": fields.Datetime.now() - datetime.timedelta(hours=hours_ago),
                "status": status,
            }
        )

    def setUp(self):
        super().setUp()
        self.config = self.env["backup.config"].create(
            {"name": "Board Partial", "engine": "kopia", "target_path": "/var/lib/odoo/backups/board_partial"}
        )

    def test_01_a_fresh_partial_snapshot_does_not_make_the_board_healthy(self):
        # Tests [@ANCHOR: backup_management:COMM_backup_board_data]
        self._snap(self.config, "old_ok", 40, "completed")
        self._snap(self.config, "new_partial", 1, "partial")
        row = self._board_row(self.config)
        self.assertEqual(row["latest_snapshot"]["snapshot_id"], "old_ok")
        self.assertTrue(row["is_stale"])

    def test_02_a_fresh_completed_snapshot_is_healthy(self):
        # Tests [@ANCHOR: backup_management:COMM_backup_board_data]
        self._snap(self.config, "old_ok", 40, "completed")
        self._snap(self.config, "new_ok", 1, "completed")
        row = self._board_row(self.config)
        self.assertEqual(row["latest_snapshot"]["snapshot_id"], "new_ok")
        self.assertFalse(row["is_stale"])
