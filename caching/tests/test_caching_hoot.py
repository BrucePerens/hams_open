# -*- coding: utf-8 -*-
# Copyright © HAMS project. AGPL-3.0-or-later.
from odoo.addons.zero_sudo.tests.common import HamsHttpCase
from odoo.tests.common import tagged


def _hoot_unit_test_error_checker(message):
    # Matches test_dx_cluster_widget_hoot.py's own checker.
    return "[HOOT]" not in message


@tagged("post_install", "-at_install")
class TestCachingHoot(HamsHttpCase):
    # toast.test.js has real coverage, and this class is the tests/test_*.py
    # runner that executes it via browser_js(), so the Hoot suite runs with
    # the rest of this module's tests.
    def test_toast_hoot_suite_passes(self):
        self.browser_js(
            "/web/tests?headless&loglevel=2&preset=desktop&timeout=15000&tag=caching_toast",
            "",
            "",
            login="admin",
            timeout=120,
            success_signal="[HOOT] Test suite succeeded",
            error_checker=_hoot_unit_test_error_checker,
        )
