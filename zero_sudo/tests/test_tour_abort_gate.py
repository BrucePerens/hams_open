# -*- coding: utf-8 -*-
# SPDX-License-Identifier: AGPL-3.0-or-later
from odoo.addons.zero_sudo.tests.common import HamsHttpCase
from odoo.tests.common import tagged


def _hoot_unit_test_error_checker(message):
    # Same checker as test_offline_store_hoot.py: a [HOOT] failure is left to hoot's own report.
    return "[HOOT]" not in message


@tagged("post_install", "-at_install", "ui", "standard")
class TestTourAbortGate(HamsHttpCase):
    # Runs tour_abort_gate.test.js (the pure gate functions, including that an alarm before
    # the tour succeeds stays fatal) in a real browser.
    def test_tour_abort_gate_hoot_suite_passes(self):
        # Tests [@ANCHOR: zero_sudo:js_describe_rejection_reason]
        # Tests [@ANCHOR: zero_sudo:js_describe_phase]
        # Tests [@ANCHOR: zero_sudo:js_abort_channel]
        self.browser_js(
            "/web/tests?headless&loglevel=2&preset=desktop&timeout=15000&tag=zero_sudo_abort_gate",
            "",
            "",
            login="admin",
            timeout=120,
            success_signal="[HOOT] Test suite succeeded",
            error_checker=_hoot_unit_test_error_checker,
        )

    def test_late_page_errors_after_tour_success_do_not_fail_the_run(self):
        """The real harness, end to end: a tour that passes and then, 2 ms later, has the page
        reject a promise with an Event. That used to print "[WATCHDOG ALARM]" through
        console.error, which Odoo's browser turns into a failed test (had_failure) even though
        the tour printed SUCCEEDED; now it is a logged WARNING notice and the test passes."""
        # Tests [@ANCHOR: zero_sudo:js_abort_channel]
        browser_logger = self._logger.getChild("browser")
        with self.assertLogs(browser_logger.name, level="WARNING") as captured:
            self.start_tour("/odoo?debug=1", "zero_sudo_abort_gate_late_event", login="admin")
        text = "\n".join(captured.output)
        self.assertIn("[WATCHDOG NOTICE]", text)
        self.assertNotIn("[WATCHDOG ALARM]", text)
        self.assertEqual(
            [r for r in captured.records if r.levelname == "ERROR"],
            [],
            "nothing after the tour's success may be logged as an error",
        )
