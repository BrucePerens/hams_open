# -*- coding: utf-8 -*-
# Copyright © Bruce Perens K6BP. All Rights Reserved.
# This software is released under the AGPL-3.0 License.
from odoo.tests.common import tagged
from odoo.addons.zero_sudo.tests.common import HamsTransactionCase
from unittest.mock import MagicMock
import pika

import json
from odoo.addons.backup_management.daemon import backup_worker

@tagged("post_install", "-at_install")
class TestTddFixes(HamsTransactionCase):
    @classmethod
    def setUpClass(cls):
        super().setUpClass()
        cls.config1 = cls.env["backup.config"].create({
            "name": "Config 1",
            "engine": "kopia",
            "target_path": "/var/lib/odoo/test_kopia1",
            "storage_type": "local",
        })
        cls.config2 = cls.env["backup.config"].create({
            "name": "Config 2",
            "engine": "kopia",
            "target_path": "/var/lib/odoo/test_kopia2",
            "storage_type": "local",
        })

    def test_action_apply_policies_overwrite(self):
        configs = self.config1 | self.config2
        with self.safe_patch_object(type(self.env["backup.config"]), "_publish_to_worker", return_value={"type": "ir.actions.act_window"}):
            res1 = configs.action_trigger_backup()
            res2 = configs.action_apply_policies()
            # If multiple records are selected, res should be True, not the dictionary
            self.assertEqual(res1, True)
            self.assertEqual(res2, True)
            
            res3 = self.config1.action_trigger_backup()
            res4 = self.config1.action_apply_policies()
            # If a single record is selected, res should be the action dictionary
            self.assertEqual(type(res3), dict)
            self.assertEqual(type(res4), dict)

    # TODO: Refactor test when removing threading in utils.py
    def test_rabbitmq_connection_close_on_exception_backup(self):
        mock_conn = MagicMock()
        mock_channel = MagicMock()
        mock_conn.channel.return_value = mock_channel
        mock_channel.basic_publish.side_effect = pika.exceptions.AMQPError("Test error")

        with self.safe_patch("pika.BlockingConnection", return_value=mock_conn):
            self.config1.action_sync_snapshots()
            self.env.cr.postcommit.run()
            
        mock_conn.close.assert_called_once()

    def test_rabbitmq_connection_close_on_exception_restore(self):
        mock_conn = MagicMock()
        mock_channel = MagicMock()
        mock_conn.channel.return_value = mock_channel
        mock_channel.basic_publish.side_effect = pika.exceptions.AMQPError("Test error")

        snapshot = self.env["backup.snapshot"].create({
            "config_id": self.config1.id,
            "snapshot_id": "test_snap",
            "start_time": "2023-01-01 10:00:00",
            "size_bytes": 1000,
            "status": "completed",
        })
        wizard = self.env["backup.restore.wizard"].create({
            "snapshot_id": snapshot.id,
            "restore_target_path": "/var/lib/odoo/test_restore",
        })

        with self.safe_patch("pika.BlockingConnection", return_value=mock_conn):
            wizard.action_restore()
            self.env.cr.postcommit.run()
            
        mock_conn.close.assert_called_once()

    def test_backup_worker_stdout_reading(self):
        mock_ch = MagicMock()
        mock_method = MagicMock()
        mock_properties = MagicMock()
        body = json.dumps({
            "job_id": 1,
            "engine": "kopia",
            "target_path": "/var/lib/odoo/test",
            "config_id": 1,
            "svc_uid": 1,
            "website_id": 1,
        })
        
        mock_proc = MagicMock()
        mock_proc.wait.return_value = 0
        
        # We need to simulate the read(1) returning characters, then empty.
        mock_proc.stdout.read.side_effect = ["h", "e", "l", "l", "o", ""]
        
        # We mock Popen
        self.safe_patch("subprocess.Popen", return_value=mock_proc)
        self.safe_patch("odoo.addons.backup_management.daemon.backup_worker._json2_call", return_value=[{}])
        
        backup_worker.execute_job(mock_ch, mock_method, mock_properties, body)
                
        # Check that read(1) was called and readline was not.
        mock_proc.stdout.read.assert_called()
        mock_proc.stdout.readline.assert_not_called()
