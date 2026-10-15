# Copyright © HAMS project.
# SPDX-License-Identifier: AGPL-3.0-or-later
from odoo.addons.zero_sudo.tests.common import HamsHttpCase
from odoo.tests.common import tagged


def _hoot_unit_test_error_checker(message):
    # Matches user_websites' test_violation_report_hoot.py.
    return "[HOOT]" not in message


@tagged("post_install", "-at_install")
class TestEdgeCacheCsrfHoot(HamsHttpCase):
    def test_edge_cache_csrf_hoot_suite_passes(self):
        # Tests [@ANCHOR: COMM_test_edge_cache_csrf_token_fetched_again_at_submit]
        self.browser_js(
            "/web/tests?headless&loglevel=2&preset=desktop&timeout=15000&tag=edge_cache",
            "",
            "",
            login="admin",
            timeout=120,
            success_signal="[HOOT] Test suite succeeded",
            error_checker=_hoot_unit_test_error_checker,
        )
