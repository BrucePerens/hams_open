# Copyright © Bruce Perens K6BP.
# SPDX-License-Identifier: AGPL-3.0-or-later
"""Refusal and argument-handling branches of generalized_monitor.execute_check that guard what a check config can make the
daemon run or call. Plain unittest, no Odoo:
    python3 pager_duty/daemon/test_check_refusals.py
"""
# Tests [@ANCHOR: daemon_execute_check]
# Tests [@ANCHOR: pager_duty:ensure_executable]
# Tests [@ANCHOR: pager_duty:polling_thread]
import json
import os
import sys
import unittest
from unittest import mock

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import generalized_monitor as gm  # noqa: E402


class CheckRefusalTests(unittest.TestCase):
    def run_check(self, check, run=None):
        run = run or mock.Mock(return_value=mock.Mock(returncode=0, stdout="true\n", stderr=""))
        with mock.patch.object(gm, "ensure_executable", side_effect=lambda name: ("/usr/bin/" + name, "")), \
                mock.patch.object(gm.subprocess, "run", run):
            return gm.execute_check(check), run

    def test_01_xmlrpc_refuses_a_private_method_and_never_calls_it(self):
        with mock.patch.object(gm.xmlrpc.client, "ServerProxy") as proxy:
            ok, msg = gm.execute_check({"type": "xmlrpc", "target": "http://rpc.example.test/", "rpc_method": "_dispatch"})
        self.assertFalse(ok)
        self.assertIn("Illegal RPC method", msg)
        self.assertEqual(proxy.return_value.method_calls, [])
        self.assertEqual(proxy.return_value._dispatch.call_args_list, [])

    def test_02_xmlrpc_calls_a_public_method_and_checks_the_expectation(self):
        with mock.patch.object(gm.xmlrpc.client, "ServerProxy") as proxy:
            proxy.return_value.ping.return_value = "pong"
            self.assertEqual(gm.execute_check({"type": "xmlrpc", "target": "http://x/", "rpc_method": "ping", "expect": "pong"}), (True, "OK"))
            self.assertEqual(gm.execute_check({"type": "xmlrpc", "target": "http://x/", "rpc_method": "ping", "expect": "nope"}), (False, "XML-RPC output mismatch"))

    def test_03_checks_that_need_a_target_refuse_without_one_and_run_nothing(self):
        for ctype, text in (("icmp", "ICMP requires target"), ("docker", "Docker check requires target container name"),
                            ("cloudflared", "Cloudflared requires target"), ("smart", "SMART check requires target device"),
                            ("file_absent", "Target required")):
            (ok, msg), run = self.run_check({"type": ctype})
            self.assertFalse(ok, ctype)
            self.assertIn(text, msg)
            run.assert_not_called()

    def test_04_pg_dump_passes_the_password_by_environment_never_in_argv_and_without_a_shell(self):
        (ok, _msg), run = self.run_check({"type": "pg_dump", "target": "db.example.test", "password": "s3cret-pw", "user": "u", "dbname": "d"})
        self.assertTrue(ok)
        argv = run.call_args.args[0]
        kwargs = run.call_args.kwargs
        self.assertNotIn("s3cret-pw", " ".join(argv))
        self.assertEqual(kwargs["env"]["PGPASSWORD"], "s3cret-pw")
        self.assertIs(kwargs["shell"], False)

    def test_05_pg_dump_without_a_password_does_not_inherit_one_from_the_check(self):
        with mock.patch.dict(os.environ, {}, clear=False):
            os.environ.pop("PGPASSWORD", None)
            (ok, _msg), run = self.run_check({"type": "pg_dump", "target": "db.example.test"})
        self.assertTrue(ok)
        self.assertNotIn("PGPASSWORD", run.call_args.kwargs["env"])

    def test_06_a_container_that_is_not_running_fails(self):
        run = mock.Mock(return_value=mock.Mock(returncode=0, stdout="false\n", stderr=""))
        (ok, msg), _ = self.run_check({"type": "docker", "target": "web"}, run)
        self.assertFalse(ok)
        self.assertIn("is not running", msg)
        (ok, _msg), run2 = self.run_check({"type": "docker", "target": "web"})
        self.assertTrue(ok)
        self.assertEqual(run2.call_args.args[0][-1], "web")

    def spool(self, data, age=10):
        opened = mock.mock_open(read_data=json.dumps(data))
        return (mock.patch.object(gm.os.path, "exists", return_value=True),
                mock.patch.object(gm.os.path, "getmtime", return_value=gm.time.time() - age),
                mock.patch("builtins.open", opened))

    def smart(self, data, target="/dev/sda", age=10):
        a, b, c = self.spool(data, age)
        with a, b, c:
            return gm.execute_check({"type": "smart", "target": target})

    def test_07_smart_refuses_a_device_that_is_not_in_the_spool_a_failed_one_and_a_stale_spool(self):
        data = {"/dev/sda": {"smart_status": {"passed": True}}, "/dev/sdb": {"smart_status": {"passed": False}}}
        self.assertEqual(self.smart(data), (True, "OK"))
        ok, msg = self.smart(data, target="/dev/sdz")
        self.assertFalse(ok)
        self.assertIn("not found in SMART spool", msg)
        ok, msg = self.smart(data, target="/dev/sdb")
        self.assertFalse(ok)
        self.assertIn("Impending disk failure", msg)
        ok, msg = self.smart(data, age=3600)
        self.assertFalse(ok)
        self.assertIn("stale", msg)

    def test_08_file_absent_fails_when_the_file_exists(self):
        with mock.patch.object(gm.os.path, "exists", return_value=True):
            self.assertEqual(gm.execute_check({"type": "file_absent", "target": "/x"}), (False, "File /x exists"))
        with mock.patch.object(gm.os.path, "exists", return_value=False):
            self.assertEqual(gm.execute_check({"type": "file_absent", "target": "/x"}), (True, "OK"))


class Stop(Exception):
    pass


class RemediationTests(unittest.TestCase):
    def run_passes(self, results, check_extra, script_exists=True, grace_start=None):
        """Runs polling_thread over `results` (True = pass); returns the Popen mock."""
        passes = iter(results)
        sleeps = []

        def sleep(seconds):
            sleeps.append(seconds)
            if len(sleeps) >= len(results):
                raise Stop

        absent = {"state": gm.pagerduty_maintenance.ABSENT, "error": ""}
        check = {"id": 9102, "name": "Remedy Check", "type": "system", "interval": 60}
        check.update(check_extra)
        with mock.patch.object(gm, "execute_check", side_effect=lambda *_a, **_k: (next(passes), "x")), \
                mock.patch.object(gm, "record_check_status"), mock.patch.object(gm, "report"), \
                mock.patch.object(gm, "auto_resolve", return_value=True), \
                mock.patch.object(gm, "is_in_maintenance", return_value=False), \
                mock.patch.object(gm, "parent_suppresses", return_value=False), \
                mock.patch.object(gm.pagerduty_maintenance, "read_flag", return_value=absent), \
                mock.patch.object(gm.time, "sleep", side_effect=sleep), \
                mock.patch.object(gm.os.path, "exists", return_value=script_exists), \
                mock.patch.object(gm.subprocess, "Popen") as popen:
            self.addCleanup(gm.FAILING_CHECKS.clear)
            self.addCleanup(gm.FAILING_NAMES.clear)
            with self.assertRaises(Stop):
                gm.polling_thread(mock.Mock(), check)
        return popen

    def test_09_the_remediation_script_runs_once_without_a_shell_after_a_healthy_then_confirmed_failing_streak(self):
        popen = self.run_passes([True, False, False, False, False], {"remediate": "/opt/fix.sh", "threshold": 2})
        popen.assert_called_once_with(["/opt/fix.sh"], shell=False)

    def test_10_the_remediation_script_does_not_run_for_a_check_that_never_was_healthy_or_a_missing_script(self):
        self.assertEqual(self.run_passes([False, False, False, False], {"remediate": "/opt/fix.sh", "threshold": 2}).call_count, 0)
        self.assertEqual(self.run_passes([True, False, False, False], {"remediate": "/opt/fix.sh", "threshold": 2}, script_exists=False).call_count, 0)
        self.assertEqual(self.run_passes([True, False, False, False], {"threshold": 2}).call_count, 0)

    def test_11_a_failure_inside_the_grace_period_is_suppressed_and_runs_no_script(self):
        popen = self.run_passes([True, False, False], {"remediate": "/opt/fix.sh", "grace": 3600})
        self.assertEqual(popen.call_count, 0)


if __name__ == "__main__":
    unittest.main()
