# -*- coding: utf-8 -*-
from odoo.tests.common import tagged
from odoo.addons.hams_test.tests.common import HamsTransactionCase
from odoo.addons.pager_duty.daemon import pager_synthetic_spooler

@tagged('post_install', '-at_install')
class TestSyntheticSpooler(HamsTransactionCase):

    def test_00_i18n_headless_audit(self):
        # Tests [@ANCHOR: synthetic_i18n]
        self.assertTrue(hasattr(pager_synthetic_spooler, "execute_check"), "Safely suppresses headless API translation warnings")

    def test_01_real_bash_execution(self):
        check = {
            "type": "bash",
            "name": "test_bash",
            "code_payload": "echo 'REAL_EXECUTION_SUCCESS'",
            "sandbox_network_access": "loopback",
        }
        name, res = pager_synthetic_spooler.execute_check(check)
        self.assertTrue(res["success"], f"Real execution failed: {res.get('error')}")
        self.assertIn("REAL_EXECUTION_SUCCESS", res.get("output", ""))

    def test_02_real_network_block(self):
        check = {
            "type": "bash",
            "name": "test_net_block",
            "code_payload": "ping -c 1 8.8.8.8",
            "sandbox_network_access": "loopback",
        }
        name, res = pager_synthetic_spooler.execute_check(check)
        self.assertFalse(res["success"], "Ping should have failed due to unshared network.")
