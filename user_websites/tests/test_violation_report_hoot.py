# -*- coding: utf-8 -*-
# This software is distributed under the terms of the Affero General Public License (AGPL-3).
from odoo.addons.zero_sudo.tests.common import HamsHttpCase
from odoo.tests.common import tagged


def _hoot_unit_test_error_checker(message):
    # Matches test_toast_notifications_hoot.py's own checker.
    return "[HOOT]" not in message


@tagged("post_install", "-at_install")
class TestViolationReportHoot(HamsHttpCase):
    # violation_report.test.js is bundled in this module's
    # web.assets_unit_tests and declares the tag
    # "user_websites_violation_report", but no browser_js() wrapper
    # anywhere in either repository ever asked /web/tests for that tag, so
    # its three tests of _onModalShow() had never once executed.
    #
    # This module already had a hoot runner (test_toast_notifications_hoot.py),
    # which is why check_hoot_runner_coverage.py's module-level check passed
    # it: that check is satisfied by a single runner, so a module can have
    # ten suites, one wrapper, and nine suites nobody ever triggers. Found
    # 2026-09-15 by the per-tag cross-reference added to that same checker,
    # which its own docstring had until then named as a real gap it did not
    # attempt.
    def test_violation_report_hoot_suite_passes(self):
        self.browser_js(
            "/web/tests?headless&loglevel=2&preset=desktop&timeout=15000&tag=user_websites_violation_report",
            "",
            "",
            login="admin",
            timeout=120,
            success_signal="[HOOT] Test suite succeeded",
            error_checker=_hoot_unit_test_error_checker,
        )
