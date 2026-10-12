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

    def test_stale_pending_job_is_failed_with_a_reason_and_fresh_ones_are_not(self):
        # Tests [@ANCHOR: backup_management:COMM_fail_stale_pending_jobs]
        # Tests [@ANCHOR: backup_management:COMM_auto_refresh_status]
        Job = self.env["backup.job"]
        old = Job.create({"config_id": self.config.id, "job_type": "kopia", "state": "pending"})
        processing = Job.create({"config_id": self.config.id, "job_type": "kopia", "state": "processing"})
        # create_date is set by the ORM; age the rows through SQL (test transaction only).
        self.env.cr.execute(
            "UPDATE backup_job SET create_date = now() at time zone 'utc' - interval '7 hours' WHERE id IN %s",
            (tuple((old | processing).ids),),
        )
        self.env.invalidate_all()
        Job._auto_refresh_status()
        self.assertEqual(old.state, "failed")
        self.assertIn("NOT run", old.output_log)
        self.assertEqual(self.job.state, "pending", "a job created just now must stay pending")
        # A job a worker already picked up is the processing timeout's business, not this one's.
        self.assertEqual(processing.state, "processing")
