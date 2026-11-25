# -*- coding: utf-8 -*-
# Copyright © Bruce Perens K6BP. All Rights Reserved. This software is proprietary and confidential.
"""Utility and confirmation pages (account-erasure confirmations) are not offered to search engines in /sitemap.xml."""
from odoo.addons.zero_sudo.tests.common import HamsTransactionCase
from odoo.tests.common import tagged

UTILITY_PATHS = ("/privacy/erased", "/privacy/erasing")


@tagged("post_install", "-at_install")
class TestSitemapErasurePages(HamsTransactionCase):
    def test_erasure_confirmation_pages_are_not_in_the_sitemap(self):
        website = self.env["website"].search([], limit=1)
        listed = {page["loc"] for page in website._enumerate_pages()}
        for path in UTILITY_PATHS:
            self.assertNotIn(path, listed)
