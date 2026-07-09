# -*- coding: utf-8 -*-

from . import common
from odoo.exceptions import AccessError
from odoo import _

import sys
from odoo.tests.common import tagged

@tagged("post_install", "-at_install")
class TestZeroSudoFixes(common.HamsTransactionCase):
    def test_short_circuit_res_users(self):
        regular = self.env["res.users"].create({
            "name": "Regular User",
            "login": "regular_user_test@example.com",
        })
        service = self.env["res.users"].create({
            "name": "Service Account",
            "login": "service_account_test@example.com",
            "is_service_account": True,
        })
        
        users = regular | service
        users.write({"name": "Updated Name"})
        
        self.assertEqual(regular.name, "Updated Name")
        self.assertEqual(service.name, "Updated Name")

    def test_ir_module_module_access_error(self):
        utils = self.env["zero_sudo.security.utils"]
        
        def mock_get_service_uid(self_inst, xmlid, raise_if_not_found=True):
            if xmlid == "zero_sudo.odoo_facility_service_internal":
                raise AccessError(_("Simulated Access Error"))
            return 1
            
        self.safe_patch_object(type(utils), '_get_service_uid', mock_get_service_uid)
            
        # This should not raise an exception
        self.env["ir.module.module"]._bootstrap_knowledge_docs()

    def test_daemon_utils_sys_paths(self):
        daemon_utils = self.env["zero_sudo.daemon.utils"]
        
        captured_env = {}
        def mock_popen(*args, **kwargs):
            captured_env.update(kwargs.get("env", {}))
            class MockProcess:
                pid = 1234
            return MockProcess()
            
        self.safe_patch("subprocess.Popen", mock_popen)
        
        daemon_utils.start_daemon_process("/dev/null")
        pythonpath = captured_env.get("PYTHONPATH", "")
        if sys.path[0]:
            self.assertIn(sys.path[0], pythonpath)
        self.assertNotEqual(pythonpath, "/usr/lib/python3/dist-packages")
