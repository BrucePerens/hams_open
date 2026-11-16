# -*- coding: utf-8 -*-
# Copyright © Bruce Perens K6BP. All Rights Reserved.
# SPDX-License-Identifier: AGPL-3.0-or-later
"""Backup job and snapshot history is kept when its configuration is deleted (Bruce, 2026-11-16)."""
import importlib.util
import os

from odoo.addons.zero_sudo.tests.common import HamsTransactionCase
from odoo.exceptions import UserError
from odoo.tests.common import tagged

_MIGRATION = os.path.join(
    os.path.dirname(__file__), "..", "migrations", "1.1", "post-snapshot-rule-own-website.py"
)


@tagged("post_install", "-at_install")
class TestHistorySurvivesConfigDelete(HamsTransactionCase):
    # [@ANCHOR: backup_management:test_history_survives_config_delete]
    def test_history_survives_config_delete(self):
        # Tests [@ANCHOR: backup_management:job_compute_tenant]
        # Tests [@ANCHOR: backup_management:snapshot_compute_tenant]
        website = self.env["website"].create({"name": "History Keep Site"})
        config = self.env["backup.config"].create(
            {"name": "History Keep", "engine": "kopia", "target_path": "/var/lib/odoo/backups/history_keep",
             "website_id": website.id}
        )
        job = self.env["backup.job"].create({"config_id": config.id, "job_type": "kopia", "state": "done"})
        snap = self.env["backup.snapshot"].create({"config_id": config.id, "snapshot_id": "snap-keep-1"})
        config.unlink()
        job.invalidate_recordset()
        snap.invalidate_recordset()
        self.assertTrue(job.exists(), "the job history is kept")
        self.assertTrue(snap.exists(), "the snapshot history is kept")
        self.assertFalse(job.config_id or snap.config_id)
        self.assertEqual(snap.website_id, website, "an orphan stays scoped to its website")
        self.assertFalse(snap.restore_command)
        wizard = self.env["backup.restore.wizard"].create({"snapshot_id": snap.id, "restore_target_path": "/var/lib/odoo/restore/history_keep"})
        with self.assertRaises(UserError):
            wizard.action_restore()

    # [@ANCHOR: backup_management:test_migration_snapshot_rule_own_website]
    def test_migration_points_the_snapshot_rule_at_its_own_website(self):
        # Tests [@ANCHOR: backup_management:migration_snapshot_rule_own_website]
        rule = self.env.ref("backup_management.rule_backup_snapshot_multi_tenant")
        rule.domain_force = (
            "[('company_id', 'in', company_ids), '|', ('config_id.website_id', '=', False), "
            "('config_id.website_id', '=', user.website_id.id)]"
        )
        spec = importlib.util.spec_from_file_location("backup_snapshot_rule_migration", _MIGRATION)
        module = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(module)
        module.migrate(self.env.cr, "1.0")
        rule.invalidate_recordset()
        self.assertNotIn("config_id", rule.domain_force)
        self.assertIn("('website_id', '=', False)", rule.domain_force)
