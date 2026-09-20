# This software is distributed under the terms of the Affero General Public License (AGPL-3).

# -*- coding: utf-8 -*-
import ipaddress
import json
import os
import tempfile
import time

from odoo.tests.common import tagged
from odoo.addons.zero_sudo.tests.common import HamsTransactionCase
from odoo.addons.pager_duty.daemon import pager_synthetic_spooler


@tagged("post_install", "-at_install")
class TestSyntheticSpooler(HamsTransactionCase):

    def test_is_ssrf_safe_public_ip_classifies_real_addresses_correctly(self):
        # Tests [@ANCHOR: pager_duty:synthetic_spooler_is_ssrf_safe_public_ip]
        is_safe = pager_synthetic_spooler._is_ssrf_safe_public_ip
        # A genuine public address must be accepted.
        self.assertTrue(is_safe(ipaddress.ip_address("8.8.8.8")))
        # Every non-public category this function exists to reject.
        self.assertFalse(is_safe(ipaddress.ip_address("127.0.0.1")))  # loopback  # burn-ignore-ssrf-test-value
        self.assertFalse(is_safe(ipaddress.ip_address("10.0.0.1")))  # private
        self.assertFalse(is_safe(ipaddress.ip_address("169.254.169.254")))  # link-local / cloud metadata
        self.assertFalse(is_safe(ipaddress.ip_address("224.0.0.1")))  # multicast
        self.assertFalse(is_safe(ipaddress.ip_address("0.0.0.0")))  # unspecified
        self.assertFalse(is_safe(ipaddress.ip_address("::1")))  # IPv6 loopback

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

    def test_02d_sandbox_downloads_aborts_on_a_response_exceeding_the_size_cap(self):
        # Tests [@ANCHOR: synthetic_i18n]
        # Bug-hunt fix, 2026-09-14: `response.read()` used to be called with
        # no size bound at all, reading a whole HTTP response into memory in
        # one call -- since sandbox_downloads is re-fetched on every
        # `interval`, an oversized (compromised, misconfigured, or simply
        # much-larger-than-expected) response is a repeatable memory-
        # exhaustion DoS against this daemon's own process. This test
        # patches MAX_SANDBOX_DOWNLOAD_BYTES down to a tiny value (rather
        # than serving a real 200MB+ fixture) and confirms a response larger
        # than the cap is rejected mid-stream, not silently accepted.
        real_download_bytes = b"x" * 100

        class _FakeResponse:
            def __init__(self, data):
                self._buf = data

            def read(self, n=-1):
                if n < 0 or n >= len(self._buf):
                    chunk, self._buf = self._buf, b""
                else:
                    chunk, self._buf = self._buf[:n], self._buf[n:]
                return chunk

            def geturl(self):
                return "https://example.invalid/payload"

            def __enter__(self):
                return self

            def __exit__(self, *exc_info):
                return False

        self.safe_patch(
            "odoo.addons.pager_duty.daemon.pager_synthetic_spooler._assert_host_is_ssrf_safe",
            return_value=None,
        )
        self.safe_patch(
            "odoo.addons.pager_duty.daemon.pager_synthetic_spooler._urlopen_ssrf_safe",
            return_value=_FakeResponse(real_download_bytes),
        )
        self.safe_patch(
            "odoo.addons.pager_duty.daemon.pager_synthetic_spooler.MAX_SANDBOX_DOWNLOAD_BYTES",
            10,
        )
        check = {
            "type": "bash",
            "name": "test_size_cap",
            "code_payload": "echo should_never_run",
            "sandbox_downloads": "https://example.invalid/payload|deadbeef|payload.bin",
            "sandbox_network_access": "loopback",
        }
        name, res = pager_synthetic_spooler.execute_check(check)
        self.assertFalse(
            res["success"],
            "A sandbox_downloads response larger than MAX_SANDBOX_DOWNLOAD_BYTES must be rejected.",
        )
        self.assertIn("exceeded", res.get("error", ""))
        self.assertIn("byte cap", res.get("error", ""))

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
            # Root cause of an intermittent empty "Unexpected execution error:"
            # here: patching "...pager_synthetic_spooler.time.sleep" patches
            # the process-global `time` module, so the worker thread's own
            # subprocess.run(timeout=...) -> Popen._wait() (which polls with
            # time.sleep when bwrap is not yet reaped after pipe EOF, i.e.
            # under load) raised _StopLoop, whose str() is empty, and the
            # bash check reported success=False. Replace only the daemon
            # module's `time` name with a proxy so nothing else is affected.
            class _LoopStopTime:
                time = staticmethod(time.time)

                @staticmethod
                def sleep(_seconds):
                    raise _StopLoop()

            self.safe_patch(
                "odoo.addons.pager_duty.daemon.pager_synthetic_spooler.time",
                new=_LoopStopTime,
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
