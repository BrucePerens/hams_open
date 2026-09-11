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

    def test_02b_sandbox_downloads_rejects_a_loopback_target(self):
        # Bug-hunt fix, 2026-09-11: sandbox_downloads' own fetch runs in
        # this daemon's parent process, BEFORE the bwrap sandboxing
        # test_02_real_network_block above exercises is ever applied --
        # sandbox_network_access="loopback" (the default) only restricts
        # the later execution step, not this download, so a malicious/
        # compromised check config could previously make this daemon fetch
        # from any internal/loopback/link-local target regardless of that
        # setting. 127.0.0.1 needs no real network access to test: a safe,
        # deterministic target that must be rejected before any connection
        # is even attempted.
        # Tests [@ANCHOR: pager_duty:synthetic_spooler_ssrf_guard]
        check = {
            "type": "bash",
            "name": "test_ssrf_block",
            "code_payload": "echo should_never_run",
            "sandbox_downloads": "http://127.0.0.1/payload|deadbeef|payload.sh",  # burn-ignore-ssrf-test-value
            "sandbox_network_access": "loopback",
        }
        name, res = pager_synthetic_spooler.execute_check(check)
        self.assertFalse(
            res["success"], "A sandbox_downloads URL targeting loopback must be rejected."
        )
        self.assertIn("non-public address", res.get("error", ""))

    def test_02c_sandbox_downloads_rejects_an_invalid_scheme_before_ssrf_check(self):
        # Sanity check: the pre-existing scheme check still runs, and runs
        # first, so a non-http(s) URL is still rejected on its own terms
        # rather than by the new SSRF guard.
        check = {
            "type": "bash",
            "name": "test_bad_scheme",
            "code_payload": "echo should_never_run",
            "sandbox_downloads": "file:///etc/passwd|deadbeef|payload.sh",
            "sandbox_network_access": "loopback",
        }
        name, res = pager_synthetic_spooler.execute_check(check)
        self.assertFalse(res["success"])
        self.assertIn("Invalid URL scheme", res.get("error", ""))

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
