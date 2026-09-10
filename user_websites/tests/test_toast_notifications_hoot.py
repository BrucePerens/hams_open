# -*- coding: utf-8 -*-
# This software is distributed under the terms of the Affero General Public License (AGPL-3).
from odoo.addons.zero_sudo.tests.common import HamsHttpCase
from odoo.tests.common import tagged


def _hoot_unit_test_error_checker(message):
    # Matches hams_com/ham_shack/tests/test_web_transceiver_hoot.py's own checker.
    return "[HOOT]" not in message


@tagged("post_install", "-at_install")
class TestToastNotificationsHoot(HamsHttpCase):
    # toast_notifications.test.js existed with real coverage
    # (AdminViolationToast's pending-reports/silent/network-failure
    # branches) but had no browser_js() runner anywhere in this module --
    # a real, previously-uncovered gap of the same shape hams_com/
    # ham_satellite/tests/test_doppler_control_hoot.py already closed for
    # its own sibling suite -- these JS-level assertions had never
    # actually been executed by a real browser, only confirmed as
    # syntactically valid. That gap is exactly how this suite's own
    # window.fetch-reassignment bug (hoot locks window.fetch read-only;
    # the fix is mockFetch()) went unnoticed until it was run for the
    # first time.
    def test_toast_notifications_hoot_suite_passes(self):
        self.browser_js(
            "/web/tests?headless&loglevel=2&preset=desktop&timeout=15000&tag=user_websites_toast_notifications",
            "",
            "",
            login="admin",
            timeout=120,
            success_signal="[HOOT] Test suite succeeded",
            error_checker=_hoot_unit_test_error_checker,
        )
