# -*- coding: utf-8 -*-
# Copyright © Bruce Perens K6BP. All Rights Reserved.
# This software is released under the AGPL-3.0 License.
from odoo.tests.common import tagged
from odoo.addons.zero_sudo.tests.common import HamsTransactionCase
from unittest.mock import MagicMock


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
            "target_path": "/var/lib/odoo/backups/test_kopia1",
            "storage_type": "local",
        })
        cls.config2 = cls.env["backup.config"].create({
            "name": "Config 2",
            "engine": "kopia",
            "target_path": "/var/lib/odoo/backups/test_kopia2",
            "storage_type": "local",
        })

    def test_action_apply_policies_overwrite(self):
        configs = self.config1 | self.config2
        ret_val = {"type": "ir.actions.act_window"}
        with self.safe_patch_object(type(self.env["backup.config"]), "_publish_to_worker", return_value=ret_val):
            res1 = configs.action_trigger_backup()
            res2 = configs.action_apply_policies()
            # If multiple records are selected, res should be True, not the dictionary
            self.assertTrue(res1)
            self.assertTrue(res2)
            
            res3 = self.config1.action_trigger_backup()
            res4 = self.config1.action_apply_policies()
            # If a single record is selected, res should be the action dictionary
            self.assertIsInstance(res3, dict)
            self.assertIsInstance(res4, dict)


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
