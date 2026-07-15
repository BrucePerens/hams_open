# -*- coding: utf-8 -*-
import os
import json
import time
from unittest.mock import MagicMock, call
from odoo.tests.common import tagged
from odoo.addons.zero_sudo.tests.common import HamsTransactionCase
from odoo.addons.backup_management.daemon import backup_worker

@tagged("post_install", "-at_install")
class TestTddBatch1(HamsTransactionCase):

    def setUp(self):
        super().setUp()
        self.mock_ch = MagicMock()
        self.mock_method = MagicMock()
        self.mock_properties = MagicMock()
        self.patcher_json2 = self.safe_patch("odoo.addons.backup_management.daemon.backup_worker._json2_call")
        self.mock_json2 = self.patcher_json2.start()
        self.patcher_popen = self.safe_patch("subprocess.Popen")
        self.mock_popen = self.patcher_popen.start()
        
        self.mock_proc = MagicMock()
        self.mock_proc.wait.return_value = 0
        self.mock_proc.stdout.read.side_effect = ["dummy log output", ""]
        self.mock_popen.return_value = self.mock_proc

    def tearDown(self):
        super().tearDown()
        self.patcher_json2.stop()
        self.patcher_popen.stop()

    def test_item1_kopia_restore_whitelist(self):
        # 1. Kopia restore path whitelist
        # Try outside base
        body_invalid = json.dumps({
            "job_id": 1,
            "engine": "restore_cmd",
            "cmd_args": ["kopia", "restore", "snap1", "/tmp/hack"],
            "config_id": 1,
            "svc_uid": 1
        })
        backup_worker.execute_job(self.mock_ch, self.mock_method, self.mock_properties, body_invalid)
        
        failed_call = None
        for c in self.mock_json2.call_args_list:
            if c.args and c.args[1] == "report_backup_failure":
                failed_call = c
        self.assertIsNotNone(failed_call)
        self.assertIn("PermissionError", failed_call.kwargs.get("message", ""))
        self.mock_json2.reset_mock()

        # Try inside base
        body_valid = json.dumps({
            "job_id": 2,
            "engine": "restore_cmd",
            "cmd_args": ["kopia", "restore", "snap1", "/var/lib/odoo/backups/valid_path"],
            "config_id": 1,
            "svc_uid": 1
        })
        backup_worker.execute_job(self.mock_ch, self.mock_method, self.mock_properties, body_valid)
        for c in self.mock_json2.call_args_list:
            if c.args and c.args[1] == "report_backup_failure":
                self.fail("Valid path triggered failure")

    def test_item2_pgbackrest_restore_alphanumeric(self):
        # 2. pgBackRest restore_cmd strict validation
        # Valid argument
        body_valid = json.dumps({
            "job_id": 1,
            "engine": "restore_cmd",
            "cmd_args": ["pgbackrest", "restore", "--stanza=valid_stanza", "--set=2023-01-01_12:34:56"],
            "config_id": 1,
            "svc_uid": 1
        })
        backup_worker.execute_job(self.mock_ch, self.mock_method, self.mock_properties, body_valid)
        for c in self.mock_json2.call_args_list:
            if c.args and c.args[1] == "report_backup_failure":
                self.fail(f"Valid arg triggered failure: {c.kwargs.get('message')}")
        
        self.mock_json2.reset_mock()
        # Invalid argument with shell meta
        body_invalid = json.dumps({
            "job_id": 2,
            "engine": "restore_cmd",
            "cmd_args": ["pgbackrest", "restore", "--stanza=valid_stanza", "--set=2023-01-01;rm"],
            "config_id": 1,
            "svc_uid": 1
        })
        backup_worker.execute_job(self.mock_ch, self.mock_method, self.mock_properties, body_invalid)
        failed_call = None
        for c in self.mock_json2.call_args_list:
            if c.args and c.args[1] == "report_backup_failure":
                failed_call = c
        self.assertIsNotNone(failed_call)
        self.assertIn("PermissionError", failed_call.kwargs.get("message", ""))

    def test_item3_kopia_flag_injection(self):
        # 3. Kopia flag injection
        body = json.dumps({
            "job_id": 1,
            "engine": "kopia",
            "target_path": "--help",
            "config_id": 1,
            "svc_uid": 1,
            "config": {"engine": "kopia"}
        })
        backup_worker.execute_job(self.mock_ch, self.mock_method, self.mock_properties, body)
        self.mock_popen.assert_called_once()
        cmd = self.mock_popen.call_args[0][0]
        # Should be ["kopia", "snapshot", "create", "--json", "--", "--help"]
        self.assertEqual(cmd, ["kopia", "snapshot", "create", "--json", "--", "--help"])

    def test_item5_timezone_data_corruption(self):
        # 5. Timezone Data Corruption
        body = json.dumps({
            "job_id": 1,
            "engine": "restore_drill",
            "script": "/opt/odoo/daemons/backup_worker/scripts/valid.py",
            "config_id": 1,
            "svc_uid": 1,
            "config": {"engine": "restore_drill"}
        })
        # Mock os.path and access to let restore_drill pass
        with self.safe_patch("os.path.realpath", return_value="/opt/odoo/daemons/backup_worker/scripts/valid.py"), \
             self.safe_patch("os.path.exists", return_value=True), \
             self.safe_patch("os.access", return_value=True):
             
             old_tz = os.environ.get("TZ")
             os.environ["TZ"] = "America/New_York"
             time.tzset()
             try:
                 backup_worker.execute_job(self.mock_ch, self.mock_method, self.mock_properties, body)
             finally:
                 if old_tz:
                     os.environ["TZ"] = old_tz
                 else:
                     del os.environ["TZ"]
                 time.tzset()
        
        write_call = None
        for c in self.mock_json2.call_args_list:
            if c.args and c.args[0] == "backup.config" and c.args[1] == "write":
                write_call = c
        
        self.assertIsNotNone(write_call)
        last_drill = write_call.kwargs["vals"]["last_drill_time"]
        # Now check if it's UTC time. It should match time.gmtime() approximately.
        utc_str = time.strftime("%Y-%m-%d %H", time.gmtime())
        self.assertTrue(last_drill.startswith(utc_str))

    def test_item8_api_latency(self):
        # 8. API Latency - expect config in payload, no read to backup.config
        body = json.dumps({
            "job_id": 1,
            "engine": "kopia",
            "target_path": "/var/lib/odoo/backups/target",
            "config_id": 1,
            "svc_uid": 1,
            "config": {
                "kopia_password": "test",
                "keep_daily": 7,
                "engine": "kopia"
            }
        })
        backup_worker.execute_job(self.mock_ch, self.mock_method, self.mock_properties, body)
        for c in self.mock_json2.call_args_list:
            if c.args and c.args[0] == "backup.config" and c.args[1] == "read":
                self.fail("Worker performed unnecessary JSON2-RPC read for config!")
        # It should still execute successfully (since we mocked Popen)
        self.mock_popen.assert_called_once()
        self.assertEqual(self.mock_popen.call_args.kwargs["env"].get("KOPIA_PASSWORD"), "test")

    def test_file_contents(self):
        # Items 4, 6, 7, 9 - File content assertions
        base_path = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
        
        # 4. Service misses credentials
        service_path = os.path.join(base_path, "daemon", "backup-worker.service")
        with open(service_path, "r") as f:
            service_content = f.read()
        self.assertIn("EnvironmentFile=/opt/hams/etc/keys/backup_worker.env", service_content)
        
        # 6. Violation of ADR 0074
        doc_path = os.path.join(base_path, "data", "documentation.html")
        with open(doc_path, "r") as f:
            doc_content = f.read()
        self.assertIn('id="COMM_UX_BACKUP_SYNC"', doc_content)
        self.assertNotIn('id="backup-sync-section"', doc_content)
        
        # 7. hooks.py: Zero-Sudo architecture violation
        hooks_path = os.path.join(base_path, "hooks.py")
        with open(hooks_path, "r") as f:
            hooks_content = f.read()
        self.assertIn("_get_service_uid", hooks_content)
        self.assertIn(".with_user(", hooks_content)
        self.assertNotIn("_get_service_env", hooks_content)
        
        # 9. Hardcoded inline CSS
        self.assertNotIn("style=", doc_content)
