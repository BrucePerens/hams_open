# SPDX-License-Identifier: AGPL-3.0-or-later
# This software is distributed under the terms of the Affero General Public License (AGPL-3).

# -*- coding: utf-8 -*-
import json
import os
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
