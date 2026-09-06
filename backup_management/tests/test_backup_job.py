# -*- coding: utf-8 -*-
# Copyright © Bruce Perens K6BP. All Rights Reserved.
# This software is released under the AGPL-3.0-or-later License.
from odoo.tests.common import tagged
from odoo.addons.zero_sudo.tests.common import HamsTransactionCase


@tagged("post_install", "-at_install")
class TestBackupJob(HamsTransactionCase):
    def setUp(self):
        super().setUp()
        self.config = self.env["backup.config"].create(
            {
                "name": "Job Test Config",
                "engine": "kopia",
                "target_path": "/var/lib/odoo/backups/job_test",
            }
        )
        self.job = self.env["backup.job"].create(
            {
                "config_id": self.config.id,
                "job_type": "kopia",
                "state": "pending",
            }
        )

    def test_append_log_accumulates_deltas_instead_of_overwriting(self):
        # Tests [@ANCHOR: backup_management:COMM_append_log]
        self.assertFalse(self.job.output_log)
        self.job.append_log("first chunk\n")
        self.assertEqual(self.job.output_log, "first chunk\n")
        self.job.append_log("second chunk\n")
        self.assertEqual(self.job.output_log, "first chunk\nsecond chunk\n")

    def test_action_refresh_status_is_a_ui_no_op_that_returns_true(self):
        # Tests [@ANCHOR: backup_management:COMM_action_refresh_status]
        self.assertTrue(self.job.action_refresh_status())
        # It must not itself mutate the job's own state -- that's
        # _auto_refresh_status()'s job (already covered separately).
        self.assertEqual(self.job.state, "pending")
