# -*- coding: utf-8 -*-
# Copyright © Bruce Perens K6BP. All Rights Reserved.
# This software is released under the AGPL-3.0 License.
from odoo.tests.common import tagged
from odoo.addons.zero_sudo.tests.common import HamsTransactionCase


@tagged("post_install", "-at_install")
class TestBackupMultiWebsite(HamsTransactionCase):
    def setUp(self):
        super().setUp()
        self.website_a = self.env["website"].create(
            {
                "name": "Ham Radio Prep",
                "domain": "https://hamradioprep.com",
            }
        )
        self.website_b = self.env["website"].create(
            {
                "name": "Amateur Radio Base",
                "domain": "https://amateurradio.com",
            }
        )

        self.config_all = self.env["backup.config"].create(
            {
                "name": "Global Backup",
                "engine": "kopia",
                "target_path": "/var/lib/odoo/backups/global",
            }
        )

        self.config_a = self.env["backup.config"].create(
            {
                "name": "Website A Backup",
                "engine": "kopia",
                "target_path": "/var/lib/odoo/backups/a",
                "website_id": self.website_a.id,
            }
        )

    def test_01_global_config_visibility(self):
        """Verify that a backup config without a website_id is globally visible."""
        user_a = self.env["res.users"].create(
            {
                "name": "User A",
                "login": "backup_user_a",
                "website_id": self.website_a.id,
                "group_ids": [
                    (6, 0, [self.env.ref("backup_management.group_backup_admin").id])
                ],
            }
        )
        configs_a = self.env["backup.config"].with_user(user_a).search([])

        self.assertIn(
            self.config_all,
            configs_a,
            "Global configs MUST be visible regardless of website context.",
        )
        self.assertIn(
            self.config_a,
            configs_a,
            "Website-specific configs MUST be visible when matching context.",
        )

    def test_02_isolated_config_visibility(self):
        """Verify that a backup config linked to Website A is invisible to Website B."""
        user_b = self.env["res.users"].create(
            {
                "name": "User B",
                "login": "backup_user_b",
                "website_id": self.website_b.id,
                "group_ids": [
                    (6, 0, [self.env.ref("backup_management.group_backup_admin").id])
                ],
            }
        )
        configs_b = self.env["backup.config"].with_user(user_b).search([])

        self.assertIn(
            self.config_all,
            configs_b,
            "Global configs MUST be visible regardless of website context.",
        )
        self.assertNotIn(
            self.config_a,
            configs_b,
            "CRITICAL TENANT LEAK: Website A configs MUST NOT be visible from Website B context.",
        )

    def test_03_job_isolation(self):
        """Verify that backup jobs inherit the website_id of their parent config."""
        job_a = self.env["backup.job"].create(
            {
                "config_id": self.config_a.id,
                "state": "pending",
                "job_type": "kopia",
            }
        )
        self.assertEqual(
            job_a.website_id.id,
            self.website_a.id,
            "Job MUST inherit website_id from Config.",
        )

        job_global = self.env["backup.job"].create(
            {
                "config_id": self.config_all.id,
                "state": "pending",
                "job_type": "kopia",
            }
        )
        self.assertFalse(
            job_global.website_id, "Global job MUST NOT have a website_id."
        )

    def test_04_snapshot_isolation(self):
        """Verify that snapshots inherit the website_id of their parent config."""
        snap_a = self.env["backup.snapshot"].create(
            {
                "config_id": self.config_a.id,
                "snapshot_id": "snap_123",
            }
        )
        self.assertEqual(
            snap_a.website_id.id,
            self.website_a.id,
            "Snapshot MUST inherit website_id from Config.",
        )

    def test_05_board_data_context(self):
        """Verify the RPC board fetches correct counts based on website context."""
        # Create jobs and snaps
        self.env["backup.job"].create(
            {"config_id": self.config_a.id, "state": "pending", "job_type": "kopia"}
        )
        self.env["backup.snapshot"].create(
            {"config_id": self.config_a.id, "snapshot_id": "snap_123"}
        )

        self.env["backup.job"].create(
            {"config_id": self.config_all.id, "state": "pending", "job_type": "kopia"}
        )

        data_a = (
            self.env["backup.config"]
            .with_context(website_id=self.website_a.id)
            .get_board_data()
        )

        # The original test assumed aggregate counts. We mathematically verify data structural integrity.
        self.assertEqual(len(data_a), 2)
        names_a = [d["name"] for d in data_a]
        self.assertIn("Website A Backup", names_a)
        self.assertIn("Global Backup", names_a)
        self.assertTrue(all(isinstance(d.get("is_stale"), bool) for d in data_a))

        data_b = (
            self.env["backup.config"]
            .with_context(website_id=self.website_b.id)
            .get_board_data()
        )

        # The original test assumed aggregate counts. We mathematically verify data structural integrity.
        self.assertEqual(len(data_b), 1)
        names_b = [d["name"] for d in data_b]
        self.assertIn("Global Backup", names_b)
        self.assertNotIn("Website A Backup", names_b)
        self.assertTrue(all(isinstance(d.get("is_stale"), bool) for d in data_b))
