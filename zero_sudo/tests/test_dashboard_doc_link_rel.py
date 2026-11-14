# Copyright © Bruce Perens K6BP. All Rights Reserved. This software is proprietary and confidential.
# -*- coding: utf-8 -*-
"""The security dashboard's help link opens a new tab and must carry rel=noopener."""
import os
import re

from odoo.addons.zero_sudo.tests.common import HamsTransactionCase
from odoo.tests.common import tagged


@tagged("post_install", "-at_install")
class TestDashboardDocLinkRel(HamsTransactionCase):
    def test_blank_links_have_noopener(self):
        path = os.path.join(
            os.path.dirname(__file__), "..", "static", "src", "components", "security_dashboard", "security_dashboard.xml"
        )
        with open(path, encoding="utf-8") as fh:
            src = fh.read()
        for tag in re.findall(r"<a\b[^>]*target=\"_blank\"[^>]*>", src):
            self.assertRegex(tag, r"rel=\"[^\"]*(noopener|noreferrer)", tag)
