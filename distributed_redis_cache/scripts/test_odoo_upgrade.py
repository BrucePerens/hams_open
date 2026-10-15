#!/usr/bin/env python3
# SPDX-License-Identifier: AGPL-3.0-or-later
"""Unit tests for odoo_upgrade.py (stdlib only; run with `python3 -m unittest test_odoo_upgrade` from this directory)."""
# Tests [@ANCHOR: distributed_redis_cache:COMM_odoo_upgrade_tool]

import contextlib
import importlib.util
import io
import os
import unittest

import odoo_upgrade as u

SHOW = """ExecStart={ path=/usr/bin/odoo }
Environment=PYTHONPYCACHEPREFIX=/opt/hams/pycache ODOO_RC=/etc/odoo/odoo.conf
EnvironmentFiles=/opt/hams/etc/odoo.env (ignore_errors=yes)
EnvironmentFiles=/opt/hams/etc/redis.env (ignore_errors=yes)
EnvironmentFiles=/opt/hams/etc/must.env (ignore_errors=no)
User=odoo
Group=odoo
"""
SECRET = "s3cr3t-value-never-printed"
FILES = {"/opt/hams/etc/odoo.env": "A=1\n", "/opt/hams/etc/redis.env": f"# c\nREDIS_USERNAME=u\nREDIS_PASSWORD={SECRET}\n",
         "/opt/hams/etc/must.env": "X=1"}


class TestOdooUpgrade(unittest.TestCase):
    def _run(self, argv, files=FILES, active=False, execute=None, show=SHOW, css_check=None):
        out = io.StringIO()
        rc = u.run(argv, show=lambda: show, read_text=lambda p: files[p], is_active=lambda: active,
                   execute=execute or (lambda cmd, log, o: 0), out=out,
                   css_check=css_check or (lambda cmd, script, o: ["ASSET_BUNDLE_CHECK: OK 5 bundle compiles\n"]),
                   list_backups=lambda: ["/opt/hams/backups/deploy-20261013-0100", "/opt/hams/backups/deploy-20261014-1500"])
        return rc, out.getvalue()

    def test_command_uses_service_environment_files_by_path_and_never_contains_values(self):
        rc, out = self._run(["--db", "hams_prod", "--modules", "a_b,c", "--dry-run"])
        self.assertEqual(rc, 0)
        self.assertIn("-p EnvironmentFile=-/opt/hams/etc/redis.env", out)
        self.assertIn("-p EnvironmentFile=/opt/hams/etc/must.env", out)  # not optional in the unit -> not optional here
        self.assertIn("-p User=odoo", out)
        self.assertIn("-E ODOO_RC=/etc/odoo/odoo.conf", out)
        self.assertIn("-u a_b,c --stop-after-init", out)
        self.assertIn("--unit=odoo-upgrade", out)  # fixed name: no second concurrent upgrade, and `systemctl is-active` can see it
        self.assertNotIn(SECRET, out)

    def test_refuses_when_no_environment_file_defines_the_redis_password(self):
        files = dict(FILES, **{"/opt/hams/etc/redis.env": "REDIS_USERNAME=u\n"})
        with self.assertRaisesRegex(u.UpgradeError, "REDIS_PASSWORD"):
            self._run(["--db", "d", "--modules", "m"], files=files)

    def test_refuses_without_any_environment_file(self):
        with self.assertRaisesRegex(u.UpgradeError, "no EnvironmentFile"):
            self._run(["--db", "d", "--modules", "m"], show="User=odoo\n")

    def test_unreadable_file_is_a_refusal_not_a_traceback(self):
        def read(path):
            raise PermissionError(path)
        with self.assertRaisesRegex(u.UpgradeError, "cannot read"):
            u.run(["--db", "d", "--modules", "m"], show=lambda: SHOW, read_text=read, is_active=lambda: False)

    def test_refuses_while_the_service_is_running(self):
        with self.assertRaisesRegex(u.UpgradeError, "running"):
            self._run(["--db", "d", "--modules", "m"], active=True)

    def test_dry_run_is_allowed_while_the_service_runs(self):
        self.assertEqual(self._run(["--db", "d", "--modules", "m", "--dry-run"], active=True)[0], 0)

    def test_rejects_unsafe_module_and_db_names(self):
        for argv in (["--db", "d", "--modules", "a;rm"], ["--db", "d;x", "--modules", "m"], ["--db", "d", "--modules", ","]):
            with self.assertRaises(u.UpgradeError):
                self._run(argv)

    def test_dry_run_executes_nothing(self):
        def boom(*a):
            raise AssertionError("executed")
        self.assertEqual(self._run(["--db", "d", "--modules", "m", "--dry-run"], execute=boom)[0], 0)

    def test_env_file_keys_ignores_comments_and_values(self):
        self.assertEqual(u.env_file_keys("# A=1\nB = 2\n\nC=3\n"), {"B", "C"})

    def test_auth_failure_in_output_fails_the_run_even_if_odoo_exited_zero(self):
        class P:
            stdout = iter(["ok\n", "WARNING Redis cache invalidation failed: Authentication required.\n"])
            def wait(self):
                return 0
        orig = u.subprocess.Popen
        u.subprocess.Popen = lambda *a, **k: P()
        try:
            self.assertEqual(u._execute(["x"], None, io.StringIO()), 3)
        finally:
            u.subprocess.Popen = orig


OK_LINES = ["noise\n", "ASSET_BUNDLE_CHECK: OK 7 bundle compiles\n"]
BAD_LINES = ["ASSET_BUNDLE_CHECK: web.assets_frontend (website 1): Error: \"calc(100vw - 16px)\" is not a number for `min'\n",
             "ASSET_BUNDLE_CHECK: FAIL 1 failure(s)\n"]


class TestStylesheetCheck(unittest.TestCase):
    def _run(self, argv=("--db", "hams_prod", "--modules", "m"), upgrade_rc=0, lines=OK_LINES, check_called=None):
        out = io.StringIO()

        def check(cmd, script, o):
            if check_called is not None:
                check_called.append((cmd, script))
            return lines
        rc = u.run(list(argv), show=lambda: SHOW, read_text=lambda p: FILES[p], is_active=lambda: False,
                   execute=lambda cmd, log, o: upgrade_rc, out=out, css_check=check,
                   list_backups=lambda: ["/opt/hams/backups/deploy-20261013-0100", "/opt/hams/backups/deploy-20261014-1500"])
        return rc, out.getvalue()

    def test_a_passing_check_leaves_the_exit_code_zero(self):
        self.assertEqual(self._run()[0], 0)

    def test_a_failing_check_exits_4_and_prints_the_failure_and_how_to_roll_back(self):
        err = io.StringIO()
        with contextlib.redirect_stderr(err):
            rc, _ = self._run(lines=BAD_LINES)
        text = err.getvalue()
        self.assertEqual(rc, u.CSS_CHECK_EXIT)
        self.assertIn("is not a number for `min'", text)
        self.assertIn("DO NOT START ODOO", text)
        self.assertIn("/opt/hams/backups/deploy-20261014-1500", text)  # the newest backup, not the older one
        self.assertNotIn("deploy-20261013-0100", text)
        self.assertIn("--site-css-only", text)

    def test_no_verdict_at_all_is_a_failure(self):
        with contextlib.redirect_stderr(io.StringIO()):
            self.assertEqual(self._run(lines=["Traceback (most recent call last):\n"])[0], u.CSS_CHECK_EXIT)

    def test_an_ok_line_followed_by_a_failure_line_fails(self):
        self.assertFalse(u.judge_css_check(OK_LINES + BAD_LINES)[0])

    def test_the_check_is_not_run_when_the_upgrade_failed_and_its_code_is_kept(self):
        called = []
        self.assertEqual(self._run(upgrade_rc=1, check_called=called)[0], 1)
        self.assertEqual(called, [])

    def test_no_css_check_skips_it(self):
        called = []
        self.assertEqual(self._run(argv=("--db", "d", "--modules", "m", "--no-css-check"), lines=BAD_LINES, check_called=called)[0], 0)
        self.assertEqual(called, [])

    def test_the_check_command_is_odoo_shell_without_http_in_its_own_unit_with_the_service_environment(self):
        called = []
        self._run(check_called=called)
        cmd, script = called[0]
        self.assertIn("--unit=odoo-upgrade-css-check", cmd)
        self.assertNotIn("--unit=odoo-upgrade", cmd)
        self.assertEqual(cmd[-7:], ["/usr/bin/odoo", "shell", "--config", "/etc/odoo/odoo.conf", "-d", "hams_prod", "--no-http"])
        self.assertIn("EnvironmentFile=-/opt/hams/etc/redis.env", cmd)
        self.assertIn("User=odoo", cmd)
        self.assertNotIn("-u", cmd)
        self.assertTrue(script.endswith("check_asset_bundles_shell.py"))

    def test_the_check_script_is_shipped_beside_the_tool(self):
        self.assertTrue(os.path.isfile(u.CSS_CHECK_SCRIPT))

    def test_dry_run_prints_the_check_command_and_runs_nothing(self):
        called = []
        rc, out = self._run(argv=("--db", "d", "--modules", "m", "--dry-run"), check_called=called)
        self.assertEqual((rc, called), (0, []))
        self.assertIn("odoo-upgrade-css-check", out)

    def test_rollback_advice_without_any_backup_says_so(self):
        self.assertIn("none found", u.rollback_advice(lambda: []))


class TestCheckScript(unittest.TestCase):
    """The piped script's own logic against a fake environment (the real compile is exercised by ham_base's Odoo test)."""

    def _load(self):
        spec = importlib.util.spec_from_file_location("check_asset_bundles_shell", u.CSS_CHECK_SCRIPT)
        module = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(module)
        return module

    def _env(self, errors, stylesheet=True):
        class Bundle:
            def __init__(self, name):
                self.css_errors = []
                self.name = name

            def preprocess_css(self):
                self.css_errors = list(errors.get(self.name, []))

        class Asset:
            def _get_asset_params(self):
                return {"website_id": False}

            def _get_asset_paths(self, name, params):
                return [("/x/a.scss" if stylesheet and name != "website.assets_wysiwyg" else "/x/a.js", None, name, None)]

        class Qweb:
            def _get_asset_bundle(self, name, css, js, assets_params):
                return Bundle(name)

        class Cursor:
            rolled = 0

            def rollback(self):
                Cursor.rolled += 1

        class Env(dict):
            cr = Cursor()

        return Env({"ir.asset": Asset(), "ir.qweb": Qweb()})

    def test_clean_bundles_report_ok_and_skip_bundles_without_a_stylesheet(self):
        module = self._load()
        runs, failures = module.check(self._env({}))
        self.assertEqual((runs, failures), (4, []))  # wysiwyg has no stylesheet here; no website model -> one run each

    def test_a_css_error_is_reported_with_the_bundle_name(self):
        module = self._load()
        runs, failures = module.check(self._env({"web.assets_frontend": ['Error: "calc(100vw - 16px)" is not a number for `min\'\n  on line 1']}))
        self.assertEqual(len(failures), 1)
        self.assertIn("web.assets_frontend", failures[0])
        self.assertIn("is not a number", failures[0])

    def test_the_script_main_prints_a_verdict_that_the_caller_accepts_or_rejects(self):
        module = self._load()
        for errors, expect in (({}, True), ({"web.assets_backend": ["boom"]}, False)):
            buffer = io.StringIO()
            with contextlib.redirect_stdout(buffer):
                module.report(self._env(errors))
            self.assertEqual(u.judge_css_check(buffer.getvalue().splitlines())[0], expect, buffer.getvalue())

    def test_an_exception_while_compiling_still_prints_a_failing_verdict(self):
        module = self._load()
        buffer = io.StringIO()
        with contextlib.redirect_stdout(buffer):
            env = self._env({})
            del env["ir.asset"]  # the compile cannot even start
            module.report(env)
        self.assertFalse(u.judge_css_check(buffer.getvalue().splitlines())[0])


if __name__ == "__main__":
    unittest.main()
