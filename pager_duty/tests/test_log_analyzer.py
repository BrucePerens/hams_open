# -*- coding: utf-8 -*-
from odoo.tests.common import TransactionCase, tagged


@tagged("standard", "post_install", "-at_install")
class TestLogAnalyzer(TransactionCase):
    def test_01_log_analyzer_views(self):
        # [@ANCHOR: test_log_analyzer_views]
        v1 = self.env["pager.log.pattern"].get_view(view_type="list")
        self.assertIn("regex", v1["arch"])

        v2 = self.env["pager.log.file"].get_view(view_type="list")
        self.assertIn("filepath", v2["arch"])

    def test_02_headless_api_translation(self):
        # Tests [@ANCHOR: pd_log_api_i18n]
        # Tests [@ANCHOR: pd_log_api_i18n]
        recs = self.env["pager.log.pattern"].search([], limit=1)
        self.assertIsNotNone(recs)
