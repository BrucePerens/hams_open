# -*- coding: utf-8 -*-
# Part of Odoo. See LICENSE file for full copyright and licensing details.
#
# This file is part of hams_open, an open source module.
# License: AGPL-3.0


from . import common
from odoo.exceptions import AccessError
from odoo import _
import odoo.tools

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

    def test_ir_http_is_service_account_cached(self):
        # [@ANCHOR: COMM_test_is_service_account_cached]
        user = self.env["res.users"].create({
            "name": "Service Account IrHttp",
            "login": "service_account_irhttp@example.com",
            "is_service_account": True,
        })
        self.assertTrue(self.env["ir.http"]._is_service_account_cached(user.id))
        
        user2 = self.env["res.users"].create({
            "name": "Regular IrHttp",
            "login": "regular_irhttp@example.com",
            "is_service_account": False,
        })
        self.assertFalse(self.env["ir.http"]._is_service_account_cached(user2.id))

    def test_res_users_filtered(self):
        # Test for models/res_users.py:45
        user = self.env["res.users"].create({
            "name": "Filtered User",
            "login": "filtered_user@example.com",
            "is_service_account": True,
        })
        user.write({"password": "new_password"})
        self.assertNotEqual(user.password, "new_password")

    def test_security_log_autovacuum(self):
        # [@ANCHOR: COMM_test_security_log_autovacuum]
        log = self.env["zero_sudo.security.log"].create({
            "reason": "cache_invalidation"
        })
        self.env["zero_sudo.security.log"].autovacuum()
        self.assertTrue(log.exists())

        cron = self.env.ref("zero_sudo.ir_cron_security_log_autovacuum")
        service_user = self.env.ref("zero_sudo.odoo_facility_service_internal")
        self.assertEqual(cron.user_id, service_user, "Cron must run as zero_sudo.odoo_facility_service_internal")

    def test_poll_health_check(self):
        # [@ANCHOR: COMM_test_poll_health_check]
        daemon_utils = self.env["zero_sudo.daemon.utils"]
        
        class MockResponse:
            status = 200
            def __enter__(self): return self
            def __exit__(self, *args): pass
            
        def mock_urlopen(*args, **kwargs):
            return MockResponse()
            
        self.safe_patch("urllib.request.urlopen", mock_urlopen)
        res = daemon_utils.poll_health_check("http://localhost:8080/health", timeout=1, interval=0.1)
        self.assertTrue(res)

    def test_security_log_immutability(self):
        # [@ANCHOR: COMM_test_security_log_immutability]
        log = self.env["zero_sudo.security.log"].create({
            "reason": "test_immutability"
        })
        # Check system group
        system_user = self.env.ref("base.user_admin")
        log_sudo = log.with_user(system_user)
        with self.assertRaises(AccessError):
            log_sudo.write({"reason": "changed"})
        with self.assertRaises(AccessError):
            log_sudo.unlink()
            
        # Check facility service group
        facility_user = self.env.ref("zero_sudo.odoo_facility_service_internal")
        log_facility = log.with_user(facility_user)
        with self.assertRaises(AccessError):
            log_facility.write({"reason": "changed_facility"})
        with self.assertRaises(AccessError):
            log_facility.unlink()

    def test_documentation_wrappers(self):
        # [@ANCHOR: COMM_test_documentation_wrappers]
        import os
        from odoo.modules.module import get_resource_path
        
        path = get_resource_path("zero_sudo", "data", "testing_documentation.html")
        with open(path, "r", encoding="utf-8") as f:
            content = f.read().strip()
        self.assertTrue(content.startswith('<div class="o_knowledge_content">'))
        self.assertTrue(content.endswith('</div>'))
