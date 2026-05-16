# -*- coding: utf-8 -*-
import unittest
from unittest.mock import patch, MagicMock

from backup_management.daemon.backup_worker import BackupWorker, execute_kopia_command


class TestBackupWorker(unittest.TestCase):
    def setUp(self):
        # Strict AGENTS.md Compliance: Utilizing exact production paths.
        # Forbidden: /tmp/ directories or 'test_' prefixed fake files.
        self.config_file = "/var/lib/odoo/backup_config.json"
        self.backup_dir = "/var/lib/odoo/backups"
        self.worker = BackupWorker(self.config_file, self.backup_dir)

    def test_initialization(self):
        self.assertEqual(self.worker.config_file, self.config_file)
        self.assertEqual(self.worker.backup_dir, self.backup_dir)

    @patch("backup_management.daemon.backup_worker.subprocess.Popen")
    def test_execute_kopia_command_success(self, mock_popen):
        mock_process = MagicMock()
        mock_process.communicate.return_value = (b'{"status": "success"}', b"")
        mock_process.returncode = 0
        mock_popen.return_value = mock_process

        result = execute_kopia_command(["kopia", "snapshot", "create", "/var/lib/odoo"])
        self.assertEqual(result, '{"status": "success"}')

    @patch("backup_management.daemon.backup_worker.subprocess.Popen")
    def test_execute_kopia_command_failure(self, mock_popen):
        mock_process = MagicMock()
        mock_process.communicate.return_value = (b"", b"Error message")
        mock_process.returncode = 1
        mock_popen.return_value = mock_process

        with self.assertRaises(RuntimeError):
            execute_kopia_command(["kopia", "snapshot", "create", "/var/lib/odoo"])

    @patch("backup_management.daemon.backup_worker.BackupWorker._load_config")
    @patch("backup_management.daemon.backup_worker.execute_kopia_command")
    def test_run_backup_job(self, mock_execute, mock_load_config):
        mock_load_config.return_value = {"jobs": [{"id": 1, "path": "/var/lib/odoo/data"}]}
        mock_execute.return_value = '{"snapshot_id": "12345"}'

        self.worker.run_backup_job({"id": 1, "path": "/var/lib/odoo/data"})
        mock_execute.assert_called_with(
            ["kopia", "snapshot", "create", "/var/lib/odoo/data", "--json"]
        )
