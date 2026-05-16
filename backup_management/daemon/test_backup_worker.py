import unittest
from unittest.mock import patch, MagicMock
import json
import os
import sys

# Mocking modules that are not available in the standalone test environment or involve Odoo
sys.modules['pika'] = MagicMock()
sys.modules['odoo'] = MagicMock()

# Now import the worker after mocking
import backup_worker

class TestBackupWorker(unittest.TestCase):
    def setUp(self):
        self.mock_ch = MagicMock()
        self.mock_method = MagicMock()
        self.mock_properties = MagicMock()

    @patch('backup_worker._json2_call')
    @patch('subprocess.Popen')
    def test_execute_job_kopia_success(self, mock_popen, mock_json_call):
        # Setup mocks
        mock_json_call.side_effect = [
            None, # write processing
            [{"kopia_password": "secret", "engine": "kopia"}], # read config
            None, # write done
            None, # action_sync_snapshots
        ]

        mock_proc = MagicMock()
        mock_proc.stdout.readline.side_effect = ["output line 1\n", ""]
        mock_proc.wait.return_value = 0
        mock_popen.return_value = mock_proc

        body = json.dumps({
            "job_id": 1,
            "engine": "kopia",
            "target_path": "/var/lib/odoo/backups/repo",
            "config_id": 10
        })

        # Execute
        backup_worker.execute_job(self.mock_ch, self.mock_method, self.mock_properties, body)

        # Verifications
        mock_popen.assert_called_once()
        args, kwargs = mock_popen.call_args
        cmd = args[0]
        self.assertIn("kopia", cmd)
        self.assertIn("snapshot", cmd)
        self.assertIn("create", cmd)
        self.assertIn("/var/lib/odoo/backups/repo", cmd)

        self.assertEqual(kwargs['env']['KOPIA_PASSWORD'], "secret")
        self.mock_ch.basic_ack.assert_called_once()

    @patch('backup_worker._json2_call')
    @patch('subprocess.Popen')
    def test_execute_job_pgbackrest_success(self, mock_popen, mock_json_call):
        # Setup mocks
        mock_json_call.side_effect = [
            None, # write processing
            [{"keep_daily": 7, "engine": "pgbackrest"}], # read config
            None, # write done
            None, # action_sync_snapshots
        ]

        mock_proc = MagicMock()
        mock_proc.stdout.readline.side_effect = ["pgbackrest output\n", ""]
        mock_proc.wait.return_value = 0
        mock_popen.return_value = mock_proc

        body = json.dumps({
            "job_id": 2,
            "engine": "pgbackrest",
            "target_path": "main",
            "config_id": 11
        })

        # Execute
        backup_worker.execute_job(self.mock_ch, self.mock_method, self.mock_properties, body)

        # Verifications
        mock_popen.assert_called_once()
        args, kwargs = mock_popen.call_args
        cmd = args[0]
        self.assertIn("pgbackrest", cmd)
        self.assertIn("backup", cmd)
        self.assertIn("--stanza=main", cmd)
        self.assertIn("--repo1-retention-full=7", cmd)

        self.mock_ch.basic_ack.assert_called_once()

    @patch('backup_worker._json2_call')
    @patch('subprocess.Popen')
    def test_execute_job_sync_kopia_parsing(self, mock_popen, mock_json_call):
        # Setup mocks
        mock_json_call.side_effect = [
            None, # write processing
            [{"engine": "kopia"}], # read config
            None, # write done
            None, # _process_snapshot_data
        ]

        json_output = json.dumps([{"id": "snap1", "startTime": "2023-01-01T12:00:00Z"}])
        mock_proc = MagicMock()
        mock_proc.stdout.readline.side_effect = [json_output + "\n", ""]
        mock_proc.wait.return_value = 0
        mock_popen.return_value = mock_proc

        body = json.dumps({
            "job_id": 3,
            "engine": "sync_snapshots",
            "target_path": "/var/lib/odoo/backups/repo",
            "config_id": 10
        })

        # Execute
        backup_worker.execute_job(self.mock_ch, self.mock_method, self.mock_properties, body)

        # Verify _process_snapshot_data was called with correct parsed data
        self.assertEqual(mock_json_call.call_count, 4)
        last_call = mock_json_call.call_args_list[-1]
        self.assertEqual(last_call[0][1], "_process_snapshot_data")
        self.assertEqual(last_call[1]['data'][0]['id'], "snap1")

    @patch('backup_worker._json2_call')
    @patch('subprocess.Popen')
    def test_execute_job_failure_reporting(self, mock_popen, mock_json_call):
        # Setup mocks
        mock_json_call.side_effect = [
            None, # write processing
            [{"engine": "kopia"}], # read config
            None, # write failed
            None, # _report_backup_failure
        ]

        mock_proc = MagicMock()
        mock_proc.stdout.readline.side_effect = ["error message\n", ""]
        mock_proc.wait.return_value = 1 # Exit code 1
        mock_popen.return_value = mock_proc

        body = json.dumps({
            "job_id": 4,
            "engine": "kopia",
            "target_path": "/var/lib/odoo/backups/repo",
            "config_id": 10
        })

        # Execute
        backup_worker.execute_job(self.mock_ch, self.mock_method, self.mock_properties, body)

        # Verify failure was reported
        self.assertEqual(mock_json_call.call_count, 4)
        self.assertEqual(mock_json_call.call_args_list[2][0][1], "write")
        self.assertEqual(mock_json_call.call_args_list[2][1]['vals']['state'], "failed")
        self.assertEqual(mock_json_call.call_args_list[3][0][1], "_report_backup_failure")

if __name__ == '__main__':
    unittest.main()
