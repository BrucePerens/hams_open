# -*- coding: utf-8 -*-
import json
import unittest
import sys
import os
from unittest.mock import MagicMock, patch

# Manifest ensures pika is present
import pika

# Add current dir to path to import backup_worker
sys.path.append(os.path.dirname(__file__))
import backup_worker

class TestBackupWorker(unittest.TestCase):
    @patch("backup_worker._json2_call")
    @patch("backup_worker.subprocess.Popen")
    def test_execute_job_kopia(self, mock_popen, mock_json):
        mock_json.side_effect = [
            {"result": "ok"}, # job write processing
            [{"kopia_password": "pass", "engine": "kopia"}], # config read
            {"result": "ok"}, # job write final
            {"result": "ok"}, # action_sync_snapshots
        ]

        mock_proc = MagicMock()
        mock_proc.stdout.readline.side_effect = ["line1\n", ""]
        mock_proc.wait.return_value = 0
        mock_popen.return_value = mock_proc

        mock_ch = MagicMock()
        mock_method = MagicMock()
        mock_method.delivery_tag = "tag1"
        body = json.dumps({
            "job_id": 1,
            "config_id": 10,
            "engine": "kopia",
            "target_path": "/var/lib/odoo/backup_repo"
        })

        backup_worker.execute_job(mock_ch, mock_method, None, body)

        mock_popen.assert_called()
        args = mock_popen.call_args[0][0]
        self.assertIn("kopia", args)
        self.assertIn("snapshot", args)
        self.assertIn("create", args)

        mock_ch.basic_ack.assert_called_with(delivery_tag="tag1")

    @patch("backup_worker._json2_call")
    @patch("backup_worker.subprocess.Popen")
    def test_execute_job_policy(self, mock_popen, mock_json):
        mock_json.side_effect = [
            {"result": "ok"}, # job write processing
            [{"keep_daily": 7, "keep_weekly": 4, "keep_monthly": 6, "exclude_patterns": "*.tmp", "engine": "kopia"}], # config read
            {"result": "ok"}, # job write final
        ]

        mock_proc = MagicMock()
        mock_proc.stdout.readline.side_effect = [""]
        mock_proc.wait.return_value = 0
        mock_popen.return_value = mock_proc

        mock_ch = MagicMock()
        mock_method = MagicMock()
        body = json.dumps({
            "job_id": 2,
            "config_id": 11,
            "engine": "kopia_policy",
            "target_path": "/var/lib/odoo/backup_repo"
        })

        backup_worker.execute_job(mock_ch, mock_method, None, body)

        args = mock_popen.call_args[0][0]
        self.assertIn("policy", args)
        self.assertIn("--keep-daily=7", args)
        self.assertIn("--add-ignore=*.tmp", args)

    @patch("backup_worker._json2_call")
    @patch("backup_worker.subprocess.Popen")
    def test_execute_job_sync(self, mock_popen, mock_json):
        mock_json.side_effect = [
            {"result": "ok"}, # job write processing
            [{"engine": "kopia"}], # config read
            {"result": "ok"}, # job write final
            {"result": "ok"}, # _process_snapshot_data
        ]

        mock_proc = MagicMock()
        mock_proc.stdout.readline.side_effect = ['[{"id": "s1"}]', ""]
        mock_proc.wait.return_value = 0
        mock_popen.return_value = mock_proc

        mock_ch = MagicMock()
        mock_method = MagicMock()
        body = json.dumps({
            "job_id": 3,
            "config_id": 12,
            "engine": "sync_snapshots",
            "target_path": "/var/lib/odoo/backup_repo"
        })

        backup_worker.execute_job(mock_ch, mock_method, None, body)

        args = mock_popen.call_args[0][0]
        self.assertIn("snapshot", args)
        self.assertIn("list", args)

        # Verify it called _process_snapshot_data with the right data
        last_call = mock_json.call_args_list[-1]
        self.assertEqual(last_call[0][1], "_process_snapshot_data")
        self.assertEqual(last_call[1]["data"], [{"id": "s1"}])

    @patch("backup_worker._json2_call")
    @patch("backup_worker.subprocess.Popen")
    def test_execute_job_pg_retention(self, mock_popen, mock_json):
        mock_json.side_effect = [
            {"result": "ok"}, # job write processing
            [{"keep_daily": 7, "engine": "pgbackrest"}], # config read
            {"result": "ok"}, # job write final
            {"result": "ok"}, # action_sync_snapshots
        ]

        mock_proc = MagicMock()
        mock_proc.stdout.readline.side_effect = [""]
        mock_proc.wait.return_value = 0
        mock_popen.return_value = mock_proc

        mock_ch = MagicMock()
        mock_method = MagicMock()
        body = json.dumps({
            "job_id": 4,
            "config_id": 13,
            "engine": "pgbackrest",
            "target_path": "main"
        })

        backup_worker.execute_job(mock_ch, mock_method, None, body)

        args = mock_popen.call_args[0][0]
        self.assertIn("--repo1-retention-full=7", args)

    @patch("backup_worker._json2_call")
    @patch("backup_worker.subprocess.Popen")
    def test_execute_job_restore_structured(self, mock_popen, mock_json):
        mock_json.side_effect = [
            {"result": "ok"}, # job write processing
            [{"engine": "kopia"}], # config read
            {"result": "ok"}, # job write final
            {"result": "ok"}, # action_sync_snapshots
        ]

        mock_proc = MagicMock()
        mock_proc.stdout.readline.side_effect = [""]
        mock_proc.wait.return_value = 0
        mock_popen.return_value = mock_proc

        mock_ch = MagicMock()
        mock_method = MagicMock()
        body = json.dumps({
            "job_id": 5,
            "config_id": 14,
            "engine": "restore_cmd",
            "cmd_args": ["kopia", "restore", "snap1", "/var/lib/odoo/backups/restore_1"],
            "snapshot_id": "snap1"
        })

        backup_worker.execute_job(mock_ch, mock_method, None, body)

        args = mock_popen.call_args[0][0]
        self.assertEqual(args, ["kopia", "restore", "snap1", "/var/lib/odoo/backups/restore_1"])

if __name__ == "__main__":
    unittest.main()
