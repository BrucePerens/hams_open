# -*- coding: utf-8 -*-
# SPDX-License-Identifier: AGPL-3.0-or-later
"""Public pages name themselves and carry a meta description (shared check: zero_sudo.tests.layout_seo)."""
from odoo.addons.zero_sudo.tests.common import HamsTransactionCase
from odoo.addons.zero_sudo.tests.layout_seo import assert_layout_seo
from odoo.tests.common import tagged

LAYOUT_TEMPLATES = [
    "caching.pwa_offline_template",
]


@tagged("post_install", "-at_install")
class TestLayoutSeo(HamsTransactionCase):
    def test_pages_have_title_and_description(self):
        assert_layout_seo(self, LAYOUT_TEMPLATES)
