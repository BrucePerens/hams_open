# -*- coding: utf-8 -*-
# Copyright © Bruce Perens K6BP.
# SPDX-License-Identifier: AGPL-3.0-or-later
from odoo.addons.zero_sudo.tests.common import HamsHttpCase
from odoo.tests.common import tagged


def _hoot_unit_test_error_checker(message):
    return "[HOOT]" not in message


@tagged("post_install", "-at_install")
class TestRichEditorHoot(HamsHttpCase):
    def test_rich_editor_hoot_suite_passes(self):
        self.browser_js(
            "/web/tests?headless&loglevel=2&preset=desktop&timeout=15000&tag=user_websites_rich_editor",
            "",
            "",
            login="admin",
            timeout=120,
            success_signal="[HOOT] Test suite succeeded",
            error_checker=_hoot_unit_test_error_checker,
        )
