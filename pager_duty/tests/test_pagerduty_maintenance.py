# SPDX-License-Identifier: AGPL-3.0-or-later

# -*- coding: utf-8 -*-
"""The pagerduty maintenance flag: the command (pager_duty/daemon/pagerduty_maintenance.py), the daemon's SMTP fallback
honouring it, and the parity of hams_shared's site monitor reader with the original. All stdlib, no Odoo records."""
import contextlib
import io
import json
import os
import stat
import sys
import tempfile
import time
from unittest.mock import MagicMock

from odoo.tests.common import tagged
from odoo.tools import mute_logger
from odoo.addons.zero_sudo.tests.common import HamsTransactionCase

import odoo.addons.pager_duty.daemon.generalized_monitor as generalized_monitor
import odoo.addons.pager_duty.daemon.pagerduty_maintenance as pm

# hams_shared sits beside this module in hams_open; site_monitor.py is stdlib-only and imports by plain name.
sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "..", "hams_shared", "tools"))
import site_monitor  # noqa: E402

NOW = 1_000_000


@tagged("post_install", "-at_install")
class TestPagerdutyMaintenance(HamsTransactionCase):
    def setUp(self):
        super().setUp()
        tmp = tempfile.TemporaryDirectory()
        self.addCleanup(tmp.cleanup)
        self.dir = tmp.name
        self.path = os.path.join(tmp.name, "maintenance")

    def _write(self, **data):
        with open(self.path, "w", encoding="utf-8") as handle:
            json.dump(data, handle)

    def _run(self, *argv, now=NOW):
        out = io.StringIO()
        code = pm.main(["--file", self.path, *argv], now=now, out=out)
        return code, out.getvalue()

    def test_01_flag_path_default_and_environment_override(self):
        # Tests [@ANCHOR: pager_duty:maintenance_flag_path]
        self.assertEqual(pm.DEFAULT_PATH, "/etc/pagerduty/maintenance")
        self.assertEqual(pm.flag_path({}), "/etc/pagerduty/maintenance")
        self.assertEqual(pm.flag_path({"PAGERDUTY_MAINTENANCE_FILE": "/x/y"}), "/x/y")
        self.assertEqual(pm.flag_path({"PAGERDUTY_MAINTENANCE_FILE": ""}), "/etc/pagerduty/maintenance")

    def test_02_read_flag_states_cap_and_malformed(self):
        # Tests [@ANCHOR: pager_duty:maintenance_read_flag]
        self.assertEqual(pm.read_flag(self.path, NOW)["state"], "absent")
        self._write(set_at=NOW, until=NOW + 600, reason="deploy", set_by="ai")
        info = pm.read_flag(self.path, NOW + 100)
        self.assertEqual((info["state"], info["remaining"], info["reason"], info["set_by"]), ("active", 500, "deploy", "ai"))
        self.assertEqual(pm.read_flag(self.path, NOW + 600)["state"], "expired")
        self._write(set_at=NOW, until=NOW + 86400)
        self.assertEqual(pm.read_flag(self.path, NOW + 7199)["state"], "active", "capped at 2 hours...")
        self.assertEqual(pm.read_flag(self.path, NOW + 7200)["state"], "expired", "...after it was set")
        for content in ("not json", "[]", '{"until": 5}', '{"set_at": "x", "until": 9}', '{"set_at": 1e999, "until": 2e999}'):
            with open(self.path, "w") as handle:
                handle.write(content)
            self.assertEqual(pm.read_flag(self.path, NOW)["state"], "malformed", content)
        self._write(set_at=NOW + 99999, until=NOW + 100999)
        self.assertEqual(pm.read_flag(self.path, NOW)["state"], "malformed", "a flag set in the future is not trusted")
        self.assertEqual(pm.read_flag(self.dir, NOW)["state"], "malformed", "unreadable counts as not in maintenance")

    def test_03_is_active_and_remove_expired(self):
        # Tests [@ANCHOR: pager_duty:maintenance_is_active]
        # Tests [@ANCHOR: pager_duty:maintenance_remove_expired]
        self._write(set_at=NOW, until=NOW + 60)
        self.assertTrue(pm.is_active(self.path, NOW))
        self.assertFalse(pm.remove_expired(self.path, NOW), "an active flag is kept")
        self.assertFalse(pm.is_active(self.path, NOW + 61))
        self.assertTrue(pm.remove_expired(self.path, NOW + 61))
        self.assertFalse(os.path.exists(self.path))
        with open(self.path, "w") as handle:
            handle.write("junk")
        self.assertFalse(pm.remove_expired(self.path, NOW), "a malformed flag is left for a human to see")
        self.assertTrue(os.path.exists(self.path))

    def test_04_write_and_clear(self):
        # Tests [@ANCHOR: pager_duty:maintenance_write_flag]
        # Tests [@ANCHOR: pager_duty:maintenance_clear_flag]
        sub = os.path.join(self.dir, "etc", "pagerduty", "maintenance")
        data = pm.write_flag(20, "why", "bruce", sub, NOW)
        self.assertEqual((data["set_at"], data["until"]), (NOW, NOW + 1200))
        self.assertEqual(stat.S_IMODE(os.stat(sub).st_mode), 0o644, "readable by the monitor units")
        self.assertEqual(stat.S_IMODE(os.stat(os.path.dirname(sub)).st_mode), 0o755)
        self.assertEqual(os.listdir(os.path.dirname(sub)), ["maintenance"], "no temporary file is left")
        for bad in (0, -5, 121):
            with self.assertRaises(ValueError):
                pm.write_flag(bad, "", "", sub, NOW)
        self.assertTrue(pm.clear_flag(sub))
        self.assertFalse(pm.clear_flag(sub))

    def test_05_command_start_status_end(self):
        # Tests [@ANCHOR: pager_duty:maintenance_main]
        # Tests [@ANCHOR: pager_duty:maintenance_describe]
        # Tests [@ANCHOR: pager_duty:maintenance_fmt_time]
        # Tests [@ANCHOR: pager_duty:maintenance_fmt_remaining]
        self.assertEqual(self._run("status")[0], 1)
        code, text = self._run("start", "--reason", "module upgrade")
        self.assertEqual(code, 0)
        self.assertIn("pagerduty maintenance started", text)
        self.assertEqual(json.load(open(self.path))["until"], NOW + 20 * 60, "default is 20 minutes")
        code, text = self._run("status", now=NOW + 90)
        self.assertEqual(code, 0)
        self.assertIn("18m30s left", text)
        self.assertIn("module upgrade", text)
        self.assertIn(time.strftime("%Y-%m-%d %H:%M:%S UTC", time.gmtime(NOW + 1200)), text)
        code, text = self._run("status", now=NOW + 1201)
        self.assertEqual(code, 1)
        self.assertIn("expired", text)
        self.assertFalse(os.path.exists(self.path), "status tidies an expired flag")
        self._run("start", "--minutes", "120")
        self.assertEqual(json.load(open(self.path))["until"], NOW + 7200)
        for bad in ("0", "121"):
            with self.assertRaises(SystemExit), contextlib.redirect_stderr(io.StringIO()):
                self._run("start", "--minutes", bad)
        self.assertEqual(json.load(open(self.path))["until"], NOW + 7200, "a refused start changes nothing")
        code, text = self._run("end")
        self.assertEqual(code, 0)
        self.assertFalse(os.path.exists(self.path))
        self.assertEqual(self._run("status")[0], 1)
        self.assertEqual(self._run("end")[0], 0, "ending twice is fine")
        with open(self.path, "w") as handle:
            handle.write("junk")
        code, text = self._run("status")
        self.assertEqual(code, 1)
        self.assertIn("unusable", text)

    def test_06_an_unprivileged_user_cannot_set_or_clear_it(self):
        # Tests [@ANCHOR: pager_duty:maintenance_main]
        locked = os.path.join(self.dir, "locked")
        os.mkdir(locked, 0o555)
        self.addCleanup(os.chmod, locked, 0o755)
        path = os.path.join(locked, "maintenance")
        err = io.StringIO()
        stderr, sys.stderr = sys.stderr, err
        try:
            code = pm.main(["--file", path, "start"], now=NOW, out=io.StringIO())
        finally:
            sys.stderr = stderr
        if os.geteuid() == 0:  # root can write anywhere: the property is then the file mode, checked in test_04
            self.assertEqual(code, 0)
        else:
            self.assertEqual(code, 2)
            self.assertIn("only root can set it", err.getvalue())
            self.assertFalse(os.path.exists(path))

    def _fallback(self, flag_path):
        mock_smtp = self.safe_patch("odoo.addons.pager_duty.daemon.generalized_monitor.smtplib.SMTP")
        mock_smtp.return_value.__enter__.return_value = MagicMock()
        saved = dict(os.environ)
        os.environ.update({"PAGER_FALLBACK_EMAIL": "a@test.com", "SMTP_HOST": "smtp.test.com",
                           "PAGERDUTY_MAINTENANCE_FILE": flag_path})
        try:
            generalized_monitor.fallback_notify("Database Check", "Odoo down", "high")
        finally:
            os.environ.clear()
            os.environ.update(saved)
        return mock_smtp

    def test_07_fallback_email_is_not_sent_during_maintenance(self):
        # Tests [@ANCHOR: pager_duty:fallback_notify]
        self._write(set_at=time.time(), until=time.time() + 600)
        with self.assertLogs("generalized_monitor", level="WARNING") as logs:
            smtp = self._fallback(self.path)
        smtp.assert_not_called()
        self.assertTrue(any("NOT sent" in line and "Odoo down" in line for line in logs.output))

    @mute_logger("generalized_monitor")
    def test_08_fallback_email_is_sent_when_maintenance_is_absent_expired_or_malformed(self):
        # Tests [@ANCHOR: pager_duty:fallback_notify]
        self._fallback(os.path.join(self.dir, "none")).assert_called_once()
        self._write(set_at=time.time() - 600, until=time.time() - 60)
        self._fallback(self.path).assert_called_once()
        self._write(set_at=time.time() - 8000, until=time.time() + 600)  # capped at 2 hours: already over
        self._fallback(self.path).assert_called_once()
        with open(self.path, "w") as handle:
            handle.write("junk")
        self._fallback(self.path).assert_called_once()

    def test_09_site_monitor_reader_agrees_with_this_one(self):
        """hams_shared's site monitor must not import this module, so it carries a copy: the two must not drift."""
        self.assertEqual(site_monitor.MAINTENANCE_DEFAULT_PATH, pm.DEFAULT_PATH)
        self.assertEqual(site_monitor.maintenance_flag_path({}), pm.flag_path({}))
        self.assertEqual(site_monitor.maintenance_flag_path({"PAGERDUTY_MAINTENANCE_FILE": "/z"}),
                         pm.flag_path({"PAGERDUTY_MAINTENANCE_FILE": "/z"}))
        self.assertEqual(site_monitor.MAINTENANCE_MAX_SECONDS, pm.MAX_SECONDS)
        cases = [dict(set_at=NOW, until=NOW + 600), dict(set_at=NOW, until=NOW + 10 ** 6), dict(until=1),
                 dict(set_at=NOW + 10 ** 5, until=NOW + 10 ** 5 + 5), dict(set_at="x", until=2), dict(set_at=NOW - 9000, until=NOW + 5)]
        for data in cases:
            self._write(**data)
            for at in (NOW, NOW + 599, NOW + 601, NOW + 7300):
                mine, theirs = pm.read_flag(self.path, at), site_monitor.read_maintenance(self.path, at)
                self.assertEqual(mine["state"], theirs["state"], (data, at))
                if mine["state"] in ("active", "expired"):
                    self.assertEqual(mine["until"], theirs["until"])
