# This software is distributed under the terms of the Affero General Public License (AGPL-3).

# -*- coding: utf-8 -*-
import json
import os
import tempfile

from odoo.tests.common import tagged
from odoo.addons.zero_sudo.tests.common import HamsTransactionCase
from odoo.addons.pager_duty.daemon import pager_synthetic_spooler


@tagged("post_install", "-at_install")
class TestSyntheticSpooler(HamsTransactionCase):

    def test_00_i18n_headless_audit(self):
        # Tests [@ANCHOR: synthetic_i18n]
        self.assertTrue(callable(pager_synthetic_spooler.execute_check))

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
        self.assertFalse(
            res["success"], "Ping should have failed due to unshared network."
        )

    def test_03_main_runs_one_real_cycle_and_writes_the_spool_file(self):
        # Tests [@ANCHOR: pager_duty:synthetic_spooler_main]
        class _StopLoop(Exception):
            pass

        with tempfile.TemporaryDirectory() as tmpdir:
            config_path = os.path.join(tmpdir, "pager_config.json")
            spool_path = os.path.join(tmpdir, "pager_synthetic_spool.json")
            with open(config_path, "w", encoding="utf-8") as f:
                json.dump(
                    {
                        "checks": [
                            {
                                "type": "bash",
                                "name": "main_test_bash",
                                "code_payload": "echo 'MAIN_LOOP_REAL_RUN'",
                                "sandbox_network_access": "loopback",
                                "interval": 60,
                            }
                        ]
                    },
                    f,
                )

            self.safe_patch(
                "odoo.addons.pager_duty.daemon.pager_synthetic_spooler.__file__",
                os.path.join(tmpdir, "pager_synthetic_spooler.py"),
            )
            self.safe_patch(
                "odoo.addons.pager_duty.daemon.pager_synthetic_spooler.SPOOL_FILE",
                spool_path,
            )
            self.safe_patch(
                "odoo.addons.pager_duty.daemon.pager_synthetic_spooler.time.sleep",
                side_effect=_StopLoop,
            )

            with self.assertRaises(_StopLoop):
                pager_synthetic_spooler.main()

            with open(spool_path, "r", encoding="utf-8") as f:
                data = json.load(f)
            self.assertTrue(data["main_test_bash"]["success"])
            self.assertIn("MAIN_LOOP_REAL_RUN", data["main_test_bash"].get("output", ""))

    def test_04_main_returns_1_when_no_config_file_exists(self):
        # Tests [@ANCHOR: pager_duty:synthetic_spooler_main]
        with tempfile.TemporaryDirectory() as tmpdir:
            self.safe_patch(
                "odoo.addons.pager_duty.daemon.pager_synthetic_spooler.__file__",
                os.path.join(tmpdir, "pager_synthetic_spooler.py"),
            )
            self.assertEqual(pager_synthetic_spooler.main(), 1)
