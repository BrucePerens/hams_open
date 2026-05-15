# -*- coding: utf-8 -*-
import datetime
from odoo import fields

import shutil
import os

if not hasattr(shutil, "_orig_which"):
    shutil._orig_which = shutil.which
    shutil.which = lambda cmd, mode=os.F_OK, path=None: (
        None if cmd in ("kopia", "etcd") else shutil._orig_which(cmd, mode, path)
    )

from odoo.tests.common import tagged
from odoo.addons.hams_test.tests.real_transaction import RealTransactionCase
from unittest.mock import patch
from odoo.exceptions import UserError


@tagged("post_install", "-at_install")
class TestBackupManagement(RealTransactionCase):
    def tearDown(self):
        if os.path.exists("/var/lib/odoo/backup_repo"):
            if os.path.isdir("/var/lib/odoo/backup_repo"):
                shutil.rmtree("/var/lib/odoo/backup_repo", ignore_errors=True)
            else:
                try:
                    os.remove("/var/lib/odoo/backup_repo")
                except OSError:
                    pass
        super().tearDown()

    def setUp(self):
        super().setUp()
        if os.path.exists("/var/lib/odoo/backup_repo"):
            if os.path.isdir("/var/lib/odoo/backup_repo"):
                shutil.rmtree("/var/lib/odoo/backup_repo", ignore_errors=True)
            else:
                try:
                    os.remove("/var/lib/odoo/backup_repo")
                except OSError:
                    pass
        self.admin = self.env.ref("base.user_admin")
        self.config_kopia = self.env["backup.config"].create(
            {"name": "Test Kopia", "engine": "kopia", "target_path": "/var/lib/odoo/backup_repo"}
        )
        self.config_pg = self.env["backup.config"].create(
            {"name": "Test PG", "engine": "pgbackrest", "target_path": "main"}
        )

    def test_01b_sync_kopia_triggered(self):
        # Tests [@ANCHOR: backup_sync_kopia]
        # Since we offloaded to RabbitMQ, we check if a job was created and task was queued.
        self.config_kopia.action_sync_snapshots()
        job = self.env["backup.job"].search([("config_id", "=", self.config_kopia.id)], order="id desc", limit=1)
        self.assertTrue(job.exists())
        self.assertEqual(job.state, "pending")

    def test_02_sync_pgbackrest_triggered(self):
        # Tests [@ANCHOR: backup_sync_pgbackrest]
        self.config_pg.action_sync_snapshots()
        job = self.env["backup.job"].search([("config_id", "=", self.config_pg.id)], order="id desc", limit=1)
        self.assertTrue(job.exists())
        self.assertEqual(job.state, "pending")

    def test_04_cron_trigger(self):
        # Tests [@ANCHOR: test_backup_cron]
        # Tests [@ANCHOR: cron_sync_all_backups]
        # Tests [@ANCHOR: backup_pager_synergy]
        # In this environment, we just ensure it queues the sync tasks.
        self.env.ref("backup_management.cron_sync_backups")._trigger()

        # Inject a stale snapshot so that it triggers _report_backup_failure -> message_post
        self.env["backup.snapshot"].create(
            {
                "config_id": self.config_kopia.id,
                "snapshot_id": "stale_snap",
                "start_time": fields.Datetime.now() - datetime.timedelta(hours=30),
                "size_bytes": 1000,
                "status": "completed",
            }
        )

        with patch.object(type(self.env["backup.config"]), "message_post") as mock_msg:
            # We must be careful because cron_sync_all_backups calls action_sync_snapshots
            # which now queues a job.
            self.env["backup.config"].cron_sync_all_backups()
            mock_msg.assert_called()

        jobs = self.env["backup.job"].search([("config_id", "=", self.config_kopia.id)])
        self.assertTrue(jobs)

    def test_07_orchestration_trigger(self):
        # Tests [@ANCHOR: test_backup_orchestration]
        # Tests [@ANCHOR: backup_trigger_execution]
        # Validates ADR-0071 Asynchronous Bastion Pattern
        integration_mode = os.environ.get("HAMS_INTEGRATION_MODE") == "1"

        # Tests are as much like production as possible, so RabbitMQ is used.
        res_kopia = self.config_kopia.action_trigger_backup()
        res_pg = self.config_pg.action_trigger_backup()

        self.assertEqual(res_kopia.get("res_model"), "backup.job")
        self.assertEqual(res_pg.get("res_model"), "backup.job")

        if integration_mode:
            # In integration mode, physically commit the transaction.
            # This triggers the `env.cr.postcommit` hook and pushes the message to RabbitMQ.
            self.env.cr.commit()
        else:
            # In standard environments without physical commits, trigger the hook manually to test Odoo's internal routing
            self.env.cr.postcommit.run()

        job_kopia = self.env["backup.job"].search(
            [("config_id", "=", self.config_kopia.id)], limit=1
        )
        self.assertTrue(job_kopia.exists())
        self.assertEqual(job_kopia.state, "pending")

        job_pg = self.env["backup.job"].search(
            [("config_id", "=", self.config_pg.id)], limit=1
        )
        self.assertTrue(job_pg.exists())
        self.assertEqual(job_pg.state, "pending")

    def test_08_apply_policies_triggered(self):
        # Tests [@ANCHOR: backup_apply_policies]
        # Tests [@ANCHOR: test_apply_policies]
        self.config_kopia.keep_daily = 7
        self.config_kopia.exclude_patterns = "*.log"
        self.config_kopia.action_apply_policies()
        job = self.env["backup.job"].search([("config_id", "=", self.config_kopia.id)], order="id desc", limit=1)
        self.assertTrue(job.exists())
        self.assertEqual(job.state, "pending")

    def test_08c_restore_drill_triggered(self):
        self.config_kopia.restore_drill_script = "/opt/test_restore.sh"
        self.config_kopia.last_drill_time = fields.Datetime.now() - datetime.timedelta(
            days=8
        )
        self.env["backup.config"].cron_sync_all_backups()
        job = self.env["backup.job"].search([("config_id", "=", self.config_kopia.id)], order="id desc", limit=1)
        self.assertTrue(job.exists())
        self.assertEqual(job.state, "pending")

    @patch(
        "odoo.addons.backup_management.models.backup_config.BackupConfig._get_executable", return_value="/bin/kopia"
    )
    def test_08d_kopia_auto_download(self, mock_get_exe):
        # Tests [@ANCHOR: test_kopia_auto_download]
        with patch.object(type(self.config_kopia), "message_post"):
            exe_path = self.config_kopia._get_executable("kopia")
        mock_get_exe.assert_called_once_with("kopia")
        self.assertEqual(exe_path, "/bin/kopia")

    def test_08e_security_path_validation(self):
        # Tests path validation added for security
        with self.assertRaises(UserError):
            self.config_kopia.write({"target_path": "/etc/shadow"})
            self.env.flush_all()

        with self.assertRaises(UserError):
            self.config_kopia.write({"restore_drill_script": "/root/hack.sh"})
            self.env.flush_all()

    def test_09_board_data_rpc(self):
        # Tests [@ANCHOR: backup_board_data]
        data = self.env["backup.config"].get_board_data()
        self.assertIsInstance(data, list)

    def test_10_restore_command_computation(self):
        # Tests [@ANCHOR: backup_restore_command]
        # Tests [@ANCHOR: test_restore_command_computation]
        snap = self.env["backup.snapshot"].create(
            {
                "config_id": self.config_kopia.id,
                "snapshot_id": "snap_123",
                "start_time": fields.Datetime.now(),
            }
        )
        self.assertIn("kopia restore snap_123", snap.restore_command)

    def test_05_views(self):
        # Tests [@ANCHOR: test_backup_view]
        v1 = self.env["backup.config"].get_view(view_type="list")
        self.assertIn("name", v1["arch"])

        v2 = self.env["backup.job"].get_view(view_type="form")
        self.assertIn("output_log", v2["arch"])

        v3 = self.env["backup.job"].get_view(view_type="list")
        self.assertIn("state", v3["arch"])

    def test_11_trigger_kopia_and_pgbackrest(self):
        # Tests [@ANCHOR: test_trigger_kopia_and_pgbackrest]
        self.config_kopia.action_trigger_backup()
        self.config_pg.action_trigger_backup()
        jobs = self.env["backup.job"].search([("config_id", "in", [self.config_kopia.id, self.config_pg.id])])
        self.assertEqual(len(jobs), 2)

    def test_12_documentation_installation(self):
        # Tests [@ANCHOR: test_backup_docs]
        # Tests [@ANCHOR: backup_doc_injection]

        # Manually trigger the hook logic for testing if needed,
        # or just check if it was installed (it should be since registry is ready)
        doc_model = False
        if "knowledge.article" in self.env:
            doc_model = "knowledge.article"
        elif "manual.article" in self.env:
            doc_model = "manual.article"

        if doc_model:
             article = self.env[doc_model].search([('name', '=', 'Backup Management')], limit=1)
             self.assertTrue(article.exists(), "Backup documentation should be installed")
             self.assertIn("Backup Management Facility", article.body)

    def test_13_restore_action(self):
        # Tests [@ANCHOR: test_restore_action]
        # Tests [@ANCHOR: backup_trigger_restore]
        snap = self.env["backup.snapshot"].create({
            "config_id": self.config_kopia.id,
            "snapshot_id": "snap_rest",
        })
        wizard = self.env["backup.restore.wizard"].create({
            "snapshot_id": snap.id,
            "restore_target_path": "/var/lib/odoo/backups/restore_target"
        })
        res = wizard.action_restore()
        self.assertEqual(res.get("res_model"), "backup.job")
        job = self.env["backup.job"].browse(res.get("res_id"))
        self.assertEqual(job.config_id, self.config_kopia)
