# -*- coding: utf-8 -*-
import os
import sys
import json
import unittest
from unittest.mock import patch, MagicMock

# Safely insert the daemon directory so sibling imports work without Odoo context
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import backup_worker  # noqa: E402


class TestBackupWorkerDaemon(unittest.TestCase):
    def setUp(self):
        self.mock_ch = MagicMock()
        self.mock_method = MagicMock()
        self.mock_method.delivery_tag = 1
        self.mock_properties = MagicMock()

    @patch("backup_worker.subprocess.Popen")
    @patch("backup_worker._json2_call")
    def test_execute_job_kopia_success(self, mock_json2, mock_popen):
        # Mock the external shell process
        mock_process = MagicMock()
        mock_process.stdout.readline.side_effect = ["Snapshot successful\n", ""]
        mock_process.wait.return_value = 0
        mock_popen.return_value = mock_process

        # Mock the Odoo API read response for the config
        mock_json2.return_value = [{"kopia_password": "test_pass", "engine": "kopia"}]

        # Construct the RabbitMQ payload
        body = json.dumps(
            {
                "job_id": 99,
                "engine": "kopia",
                "target_path": "/var/lib/odoo/backups",
                "config_id": 1,
            }
        )

        # Execute the procedural job
        backup_worker.execute_job(
            self.mock_ch, self.mock_method, self.mock_properties, body
        )

        # Assert process was spawned with correct arguments
        mock_popen.assert_called_once()
        args, kwargs = mock_popen.call_args
        self.assertEqual(
            args[0], ["kopia", "snapshot", "create", "/var/lib/odoo/backups", "--json"]
        )

        # Assert RabbitMQ acknowledgment was sent
        self.mock_ch.basic_ack.assert_called_once_with(delivery_tag=1)


if __name__ == "__main__":
    unittest.main()
