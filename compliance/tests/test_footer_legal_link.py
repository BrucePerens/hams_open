# -*- coding: utf-8 -*-
# Copyright © Bruce Perens K6BP.
# SPDX-License-Identifier: AGPL-3.0-or-later
"""The footer carries one legal link, to the /compliance hub (Bruce, 2026-11-19)."""
from lxml import html as lxml_html

from odoo.addons.zero_sudo.tests.common import HamsHttpCase
from odoo.tests.common import tagged

REMOVED = ("/privacy", "/cookie-policy", "/terms", "/accessibility", "/my/privacy")


@tagged("post_install", "-at_install")
class TestFooterLegalLink(HamsHttpCase):
    # Tests [@ANCHOR: COMM_compliance_footer_links]
    def test_footer_has_exactly_one_legal_link_to_the_hub(self):
        self.authenticate(None, None)
        tree = lxml_html.fromstring(self.url_open("/compliance").text)
        links = tree.xpath("//footer//a[@href='/compliance']")
        self.assertEqual([a.text_content().strip() for a in links], ["Legal and Compliance Information"])
        for href in REMOVED:
            self.assertEqual(tree.xpath(f"//footer//a[@href='{href}']"), [], f"footer still links {href}")
