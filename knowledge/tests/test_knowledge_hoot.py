# Copyright © Bruce Perens K6BP.
# SPDX-License-Identifier: AGPL-3.0-or-later

# -*- coding: utf-8 -*-
from odoo.addons.zero_sudo.tests.common import HamsHttpCase
from odoo.tests.common import tagged


def _hoot_unit_test_error_checker(message):
    # Matches test_dx_cluster_widget_hoot.py's own checker.
    return "[HOOT]" not in message


@tagged("post_install", "-at_install")
class TestKnowledgeHoot(HamsHttpCase):
    # Runs manual_toc.test.js (Hoot tag knowledge_manual_toc) via
    # browser_js(), so that suite is executed by the regular
    # tests/test_*.py runner instead of only by hand in the browser.
    def test_manual_toc_hoot_suite_passes(self):
        self.browser_js(
            "/web/tests?headless&loglevel=2&preset=desktop&timeout=15000&tag=knowledge_manual_toc",
            "",
            "",
            login="admin",
            timeout=120,
            success_signal="[HOOT] Test suite succeeded",
            error_checker=_hoot_unit_test_error_checker,
        )
