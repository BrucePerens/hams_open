# -*- coding: utf-8 -*-
# SPDX-License-Identifier: AGPL-3.0-or-later
"""Test waits scale with the machine's load instead of assuming an idle box."""
from odoo.addons.zero_sudo.tests import common
from odoo.addons.zero_sudo.tests.common import HamsTransactionCase
from odoo.tests.common import tagged


@tagged("post_install", "-at_install")
class TestLoadFactor(HamsTransactionCase):
    def test_idle_box_keeps_the_base_timeouts(self):
        # Tests [@ANCHOR: zero_sudo:load_factor_scaling]
        self.safe_patch_object(common.os, "getloadavg", lambda: (0.5, 0, 0))
        self.safe_patch_object(common.os, "cpu_count", lambda: 32)
        self.assertEqual(common.load_factor(), 1)

    def test_load_scales_timeouts_per_cpu_rounded_up(self):
        # Tests [@ANCHOR: zero_sudo:load_factor_scaling]
        self.safe_patch_object(common.os, "getloadavg", lambda: (75.0, 0, 0))
        self.safe_patch_object(common.os, "cpu_count", lambda: 32)
        self.assertEqual(common.load_factor(), 3)

    def test_cdp_call_timeout_is_scaled(self):
        # Tests [@ANCHOR: zero_sudo:load_factor_scaling]
        seen = {}

        def fake_original(self_, method, **kwargs):
            seen.update(kwargs)

        self.safe_patch_object(common, "original_websocket_request", fake_original)
        self.safe_patch_object(common, "load_factor", lambda: 3)
        common._patched_websocket_request(object(), "Page.enable")
        self.assertEqual(seen["timeout"], 30.0)
        common._patched_websocket_request(object(), "Runtime.evaluate", timeout=5)
        self.assertEqual(seen["timeout"], 15)
