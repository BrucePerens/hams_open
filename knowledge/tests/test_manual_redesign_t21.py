# Copyright © Bruce Perens K6BP.
# SPDX-License-Identifier: AGPL-3.0-or-later

# -*- coding: utf-8 -*-
"""Redesign T21: the manual page has one title at the 2 rem scale, a sticky sidebar tied to the header height, initials instead of emoji in the
sidebar, and feedback buttons that say what they mean. The template is read as shipped."""
from odoo.addons.zero_sudo.tests.common import HamsTransactionCase
from odoo.tests.common import tagged
from odoo.tools import file_path


@tagged("post_install", "-at_install")
class TestManualRedesignT21(HamsTransactionCase):
    def _template(self):
        with open(file_path("knowledge/views/knowledge_article_templates.xml"), encoding="utf-8") as source:
            return source.read()

    def test_01_one_title_sticky_columns_and_initial_icons(self):
        # Tests [@ANCHOR: test_manual_templates_rendering]
        text = self._template()
        self.assertNotIn("display-4", text)
        self.assertIn('<h1 class="fs-2 fw-bold">', text)
        self.assertNotIn("top: 80px", text)
        self.assertIn("var(--hams-header-h, 80px)", text)
        self.assertIn("hams-manual-icon", text)
        self.assertIn("border-start border-3 border-primary", text)

    def test_02_feedback_buttons_carry_words_and_keep_their_names(self):
        # Tests [@ANCHOR: test_tour_manual_basic_browsing]
        text = self._template()
        self.assertIn("Yes, it helped", text)
        self.assertIn("No, it did not", text)
        self.assertIn('name="is_helpful" value="1"', text)
        self.assertIn('name="is_helpful" value="0"', text)
