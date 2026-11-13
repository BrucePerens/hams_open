# SPDX-License-Identifier: AGPL-3.0-or-later

# -*- coding: utf-8 -*-
import json
import os
import subprocess
import tempfile
from unittest.mock import MagicMock

from odoo.tests.common import tagged
from odoo.addons.zero_sudo.tests.common import HamsTransactionCase
from odoo.addons.pager_duty.daemon import pager_smart_spooler


@tagged("post_install", "-at_install")
class TestPagerSmartSpooler(HamsTransactionCase):
    def test_01_generate_smart_spool_writes_health_data_for_each_device(self):
        # Tests [@ANCHOR: pager_duty:generate_smart_spool]
        # A real, additive env var (restored in cleanup), not a wholesale
        # patch of os.environ itself -- os is a single shared module
        # object, so patching "pager_smart_spooler.os.environ" would
        # replace the real process environment for every other piece of
        # code running in this same test process, not just this module.
        orig_env = dict(os.environ)
        self.addCleanup(lambda: os.environ.clear() or os.environ.update(orig_env))

        with tempfile.TemporaryDirectory() as tmpdir:
            spool_path = os.path.join(tmpdir, "pager_smart_spool.json")
            os.environ["HAMS_SMART_SPOOL_PATH"] = spool_path

            def _run(cmd, **kwargs):
                res = MagicMock()
                if "--scan" in cmd:
                    res.stdout = json.dumps({"devices": [{"name": "/dev/sda"}]})
                else:
                    res.stdout = json.dumps({"smart_status": {"passed": True}})
                return res

            self.safe_patch(
                "odoo.addons.pager_duty.daemon.pager_smart_spooler.subprocess.run",
                side_effect=_run,
            )

            pager_smart_spooler.generate_smart_spool()

            with open(spool_path, "r", encoding="utf-8") as f:
                data = json.load(f)
            self.assertIn("/dev/sda", data)
            self.assertTrue(data["/dev/sda"]["smart_status"]["passed"])

    def test_02_generate_smart_spool_exits_nonzero_on_a_real_failure(self):
        # Tests [@ANCHOR: pager_duty:generate_smart_spool]
        self.safe_patch(
            "odoo.addons.pager_duty.daemon.pager_smart_spooler.subprocess.run",
            side_effect=OSError("smartctl not found"),
        )
        with self.assertRaises(SystemExit) as ctx:
            pager_smart_spooler.generate_smart_spool()
        self.assertEqual(ctx.exception.code, 1)

    def test_03_a_hung_disk_is_skipped_not_fatal(self):
        # Tests [@ANCHOR: pager_duty:generate_smart_spool]
        orig_env = dict(os.environ)
        self.addCleanup(lambda: os.environ.clear() or os.environ.update(orig_env))
        seen_timeouts = []
        with tempfile.TemporaryDirectory() as tmpdir:
            spool_path = os.path.join(tmpdir, "pager_smart_spool.json")
            os.environ["HAMS_SMART_SPOOL_PATH"] = spool_path

            def _run(cmd, **kwargs):
                seen_timeouts.append(kwargs.get("timeout"))
                res = MagicMock()
                if "--scan" in cmd:
                    res.stdout = json.dumps({"devices": [{"name": "/dev/sda"}, {"name": "/dev/sdb"}]})
                elif "/dev/sda" in cmd:
                    raise subprocess.TimeoutExpired(cmd, kwargs.get("timeout"))
                else:
                    res.stdout = json.dumps({"smart_status": {"passed": True}})
                return res

            self.safe_patch(
                "odoo.addons.pager_duty.daemon.pager_smart_spooler.subprocess.run",
                side_effect=_run,
            )
            pager_smart_spooler.generate_smart_spool()
            with open(spool_path, "r", encoding="utf-8") as f:
                data = json.load(f)
            self.assertEqual(list(data), ["/dev/sdb"])
            self.assertNotIn(None, seen_timeouts)

    def test_04_a_failed_write_leaves_no_temp_file(self):
        # Tests [@ANCHOR: pager_duty:generate_smart_spool]
        orig_env = dict(os.environ)
        self.addCleanup(lambda: os.environ.clear() or os.environ.update(orig_env))
        with tempfile.TemporaryDirectory() as tmpdir:
            os.environ["HAMS_SMART_SPOOL_PATH"] = os.path.join(tmpdir, "pager_smart_spool.json")

            def _run(cmd, **kwargs):
                res = MagicMock()
                res.stdout = json.dumps({"devices": [{"name": "/dev/sda"}]} if "--scan" in cmd else {})
                return res

            self.safe_patch(
                "odoo.addons.pager_duty.daemon.pager_smart_spooler.subprocess.run", side_effect=_run)
            self.safe_patch("odoo.addons.pager_duty.daemon.pager_smart_spooler.os.rename", side_effect=OSError("boom"))
            with self.assertRaises(SystemExit):
                pager_smart_spooler.generate_smart_spool()
            self.assertEqual(os.listdir(tmpdir), [])
