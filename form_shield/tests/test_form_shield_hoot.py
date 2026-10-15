# SPDX-License-Identifier: AGPL-3.0-or-later
from odoo.addons.zero_sudo.tests.common import HamsHttpCase
from odoo.tests.common import tagged


def _hoot_unit_test_error_checker(message):
    # Matches user_websites' test_violation_report_hoot.py.
    return "[HOOT]" not in message


@tagged("post_install", "-at_install", "form_shield")
class TestFormShieldHoot(HamsHttpCase):
    def test_form_shield_hoot_suite_passes(self):
        # Tests [@ANCHOR: form_shield:test_pow_core_js_solves_the_shared_vector]
        self.browser_js(
            "/web/tests?headless&loglevel=2&preset=desktop&timeout=15000&tag=form_shield",
            "",
            "",
            login="admin",
            timeout=120,
            success_signal="[HOOT] Test suite succeeded",
            error_checker=_hoot_unit_test_error_checker,
        )
