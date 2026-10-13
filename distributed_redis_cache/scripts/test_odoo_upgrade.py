#!/usr/bin/env python3
# SPDX-License-Identifier: AGPL-3.0-or-later
"""Unit tests for odoo_upgrade.py (stdlib only; run with `python3 -m unittest test_odoo_upgrade` from this directory)."""
# Tests [@ANCHOR: distributed_redis_cache:COMM_odoo_upgrade_tool]

import io
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
    def _run(self, argv, files=FILES, active=False, execute=None, show=SHOW):
        out = io.StringIO()
        rc = u.run(argv, show=lambda: show, read_text=lambda p: files[p], is_active=lambda: active,
                   execute=execute or (lambda cmd, log, o: 0), out=out)
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


if __name__ == "__main__":
    unittest.main()
