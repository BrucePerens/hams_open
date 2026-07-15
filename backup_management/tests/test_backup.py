# -*- coding: utf-8 -*-
# Copyright © Bruce Perens K6BP. All Rights Reserved.
# This software is released under the AGPL-3.0 License.
import datetime
import shutil
import os
import logging

from odoo import fields, _
from odoo.tests.common import tagged
from odoo.addons.zero_sudo.tests.real_transaction import RealTransactionCase
from odoo.exceptions import UserError

_logger = logging.getLogger(__name__)


@tagged("post_install", "-at_install")
class TestBackupManagement(RealTransactionCase):
    def tearDown(self):
        if hasattr(self, 'config_kopia') and self.config_kopia.exists():
            self.config_kopia.unlink()
        if hasattr(self, 'config_pg') and self.config_pg.exists():
            self.config_pg.unlink()
        self.env.cr.commit()
        if os.path.exists("/var/lib/odoo/backup_repo"):
            if os.path.isdir("/var/lib/odoo/backup_repo"):
                shutil.rmtree("/var/lib/odoo/backup_repo")
            else:
                os.remove("/var/lib/odoo/backup_repo")
        super().tearDown()

    def setUp(self):
        super().setUp()

        # Safely isolate the shutil mock to strictly the current test instance Context
        orig_which = shutil.which

        def mock_which(cmd, mode=os.F_OK, path=None):
            if cmd in ("kopia", "etcd"):
                return None
            return orig_which(cmd, mode, path)

        self.safe_patch("shutil.which", side_effect=mock_which)

        self.env.user.write(
            {
                "group_ids": [
                    (4, self.env.ref("backup_management.group_backup_admin").id)
                ]
            }
        )
        if os.path.exists("/var/lib/odoo/backup_repo"):
            if os.path.isdir("/var/lib/odoo/backup_repo"):
                shutil.rmtree("/var/lib/odoo/backup_repo")
            else:
                os.remove("/var/lib/odoo/backup_repo")
        self.admin = self.env.ref("base.user_admin")
        self.config_kopia = self.env["backup.config"].create(
            {
                "name": "Test Kopia",
                "engine": "kopia",
                "target_path": "/var/lib/odoo/backup_repo",
            }
        )
        self.config_pg = self.env["backup.config"].create(
            {"name": "Test PG", "engine": "pgbackrest", "target_path": "main"}
        )

    def test_01b_sync_kopia_triggered(self):
        # Tests [@ANCHOR: backup_management:backup_sync_kopia]

        # Tests [@ANCHOR: backup_management:COMM_upsert_snapshots_procedure]

        # Tests [@ANCHOR: backup_management:upsert_snapshots_roundtrip_optimization]
        self.config_kopia.action_sync_snapshots()
        job = self.env["backup.job"].search(
            [("config_id", "=", self.config_kopia.id)], order="id desc", limit=1
        )
        msg = (
            "[!] DIAGNOSTIC FOR AI: Kopia sync did not create a backup.job record. "
            "Ensure action_sync_snapshots() correctly calls _publish_to_worker()."
        )
        self.assertTrue(job.exists(), msg)
        self.assertEqual(job.state, "pending")

    def test_02_sync_pgbackrest_triggered(self):
        # Tests [@ANCHOR: backup_management:backup_sync_pgbackrest]
        self.config_pg.action_sync_snapshots()
        job = self.env["backup.job"].search(
            [("config_id", "=", self.config_pg.id)], order="id desc", limit=1
        )
        msg = (
            "[!] DIAGNOSTIC FOR AI: pgBackRest sync did not create a backup.job record. "
            "Check action_sync_snapshots() logic."
        )
        self.assertTrue(job.exists(), msg)
        self.assertEqual(job.state, "pending")

    def test_04_cron_trigger(self):
        # Tests [@ANCHOR: backup_management:test_backup_cron]

        # Tests [@ANCHOR: backup_management:cron_sync_all_backups]

        # Tests [@ANCHOR: backup_management:backup_pager_synergy]
        self.env.ref("backup_management.cron_sync_backups")._trigger()

        self.env["backup.snapshot"].create(
            {
                "config_id": self.config_kopia.id,
                "snapshot_id": "stale_snap",
                "start_time": fields.Datetime.now() - datetime.timedelta(hours=30),
                "size_bytes": 1000,
                "status": "completed",
            }
        )

        self.config_kopia.message_post(body=_("Verifying failure reporting mechanism"))

        messages_before = self.env["mail.message"].search_count([("model", "=", "backup.config"), ("res_id", "=", self.config_kopia.id)])
        self.env["backup.config"].cron_sync_all_backups()
        messages_after = self.env["mail.message"].search_count([("model", "=", "backup.config"), ("res_id", "=", self.config_kopia.id)])
        self.assertGreater(messages_after, messages_before)

        jobs = self.env["backup.job"].search([("config_id", "=", self.config_kopia.id)], limit=100)
        self.assertTrue(jobs)

    def test_07_orchestration_trigger(self):
        # Tests [@ANCHOR: backup_management:test_backup_orchestration]

        # Tests [@ANCHOR: backup_management:backup_trigger_execution]
        res_kopia = self.config_kopia.action_trigger_backup()
        res_pg = self.config_pg.action_trigger_backup()

        msg_kopia = (
            "[!] DIAGNOSTIC FOR AI: action_trigger_backup() for Kopia should return "
            "an ir.actions.act_window for backup.job."
        )
        self.assertEqual(res_kopia.get("res_model"), "backup.job", msg_kopia)
        msg_pg = (
            "[!] DIAGNOSTIC FOR AI: action_trigger_backup() for pgBackRest should return "
            "an ir.actions.act_window for backup.job."
        )
        self.assertEqual(res_pg.get("res_model"), "backup.job", msg_pg)

        self.env.cr.commit()

        job_kopia = self.env["backup.job"].search(
            [("config_id", "=", self.config_kopia.id)], limit=1
        )
        msg_k_job = (
            "[!] DIAGNOSTIC FOR AI: Kopia orchestration trigger failed to create "
            "a job record."
        )
        self.assertTrue(job_kopia.exists(), msg_k_job)
        self.assertEqual(job_kopia.state, "pending")

        job_pg = self.env["backup.job"].search(
            [("config_id", "=", self.config_pg.id)], limit=1
        )
        msg_pg_job = (
            "[!] DIAGNOSTIC FOR AI: pgBackRest orchestration trigger failed to create "
            "a job record."
        )
        self.assertTrue(job_pg.exists(), msg_pg_job)
        self.assertEqual(job_pg.state, "pending")

    def test_08_apply_policies_triggered(self):
        # Tests [@ANCHOR: backup_management:backup_apply_policies]

        # Tests [@ANCHOR: test_apply_policies]
        self.config_kopia.keep_daily = 7
        self.config_kopia.exclude_patterns = "*.log"
        self.config_kopia.action_apply_policies()
        job = self.env["backup.job"].search(
            [("config_id", "=", self.config_kopia.id)], order="id desc", limit=1
        )
        self.assertTrue(job.exists())
        self.assertEqual(job.state, "pending")

    def test_08c_restore_drill_triggered(self):
        self.config_kopia.restore_drill_script = "/opt/hams/backup/test_restore.sh"
        self.config_kopia.last_drill_time = fields.Datetime.now() - datetime.timedelta(
            days=8
        )
        self.env["backup.config"].cron_sync_all_backups()
        job = self.env["backup.job"].search(
            [("config_id", "=", self.config_kopia.id)], order="id desc", limit=1
        )
        self.assertTrue(job.exists())
        self.assertEqual(job.state, "pending")

    def test_08d_kopia_auto_download(self):
        # Tests [@ANCHOR: test_kopia_auto_download]
        mock_ensure = self.safe_patch_object(
            type(self.env["binary.manifest"]), "ensure_executable", return_value="/opt/kopia"
        )

        self.config_kopia.message_post(body=_("Simulating executable resolution logs"))

        exe_path = self.config_kopia._get_executable("kopia")
        mock_ensure.assert_called_once_with("kopia")
        self.assertEqual(exe_path, "/opt/kopia")

    def test_08e_security_path_validation(self):
        with self.assertRaises(UserError):
            self.config_kopia.write({"target_path": "/etc/shadow"})
            self.env.flush_all()

        with self.assertRaises(UserError):
            self.config_kopia.write({"restore_drill_script": "/root/hack.sh"})
            self.env.flush_all()

    def test_09_board_data_rpc(self):
        # Tests [@ANCHOR: backup_management:backup_board_data]

        # Tests [@ANCHOR: backup_management:action_test_connection]

        # Tests [@ANCHOR: backup_management:action_view_latest_job]

        # Tests [@ANCHOR: backup_management:auto_refresh_status]
        data = self.env["backup.config"].get_board_data()
        self.assertIsInstance(data, list)

        # Test action_test_connection
        self.config_kopia.action_test_connection()

        # Test action_view_latest_job
        self.config_kopia.action_view_latest_job()

        # Test auto_refresh_status
        self.env["backup.job"]._auto_refresh_status()

    def test_10_restore_command_computation(self):
        # Tests [@ANCHOR: backup_management:backup_restore_command]

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
        # Tests [@ANCHOR: backup_management:test_backup_view]
        v1 = self.env["backup.config"].get_view(view_type="list")
        self.assertIn("name", v1["arch"])

        v2 = self.env["backup.job"].get_view(view_type="form")
        self.assertIn("output_log", v2["arch"])

        v3 = self.env["backup.job"].get_view(view_type="list")
        self.assertIn("state", v3["arch"])

        v4 = self.env["backup.restore.wizard"].get_view(view_type="form")
        self.assertIn("restore_target_path", v4["arch"])

    def test_11_trigger_kopia_and_pgbackrest(self):
        # Tests [@ANCHOR: test_trigger_kopia_and_pgbackrest]
        self.config_kopia.action_trigger_backup()
        self.config_pg.action_trigger_backup()
        jobs = self.env["backup.job"].search(
            [("config_id", "in", [self.config_kopia.id, self.config_pg.id])], limit=100
        )
        self.assertEqual(len(jobs), 2)

    def test_12_documentation_installation(self):
        # Tests [@ANCHOR: backup_doc_injection]
        article = self.env["knowledge.article"].search(
            [("name", "=", "Backup Management")], limit=1
        )
        msg_doc = (
            "[!] DIAGNOSTIC FOR AI: Backup documentation article 'Backup Management' not found. "
            "Verify the 'knowledge_docs' manifest entry and the bootstrap mechanism."
        )
        self.assertTrue(article.exists(), msg_doc)
        msg_body = (
            "[!] DIAGNOSTIC FOR AI: Backup documentation body content is missing "
            "expected headers. Check data/documentation.html."
        )
        self.assertIn("Backup Management User Guide", article.body, msg_body)

    def test_13_restore_action(self):
        # Tests [@ANCHOR: backup_management:COMM_test_restore_action]

        # Tests [@ANCHOR: backup_management:COMM_backup_trigger_restore]
        self.safe_patch("pika.BlockingConnection")  # burn-ignore-pika
        snap = self.env["backup.snapshot"].create(
            {
                "config_id": self.config_kopia.id,
                "snapshot_id": "snap_rest",
            }
        )
        wizard = self.env["backup.restore.wizard"].create(
            {
                "snapshot_id": snap.id,
                "restore_target_path": "/var/lib/odoo/backups/restore_target",
            }
        )
        res = wizard.action_restore()
        self.assertEqual(res.get("res_model"), "backup.job")
        job = self.env["backup.job"].browse(res.get("res_id"))
        self.assertEqual(job.config_id, self.config_kopia)
