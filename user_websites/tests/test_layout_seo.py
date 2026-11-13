# -*- coding: utf-8 -*-
# SPDX-License-Identifier: AGPL-3.0-or-later
"""Public pages name themselves and carry a meta description (shared check: zero_sudo.tests.layout_seo)."""
from odoo.addons.zero_sudo.tests.common import HamsTransactionCase
from odoo.addons.zero_sudo.tests.layout_seo import assert_layout_seo
from odoo.tests.common import tagged

LAYOUT_TEMPLATES = [
    "user_websites.blog_index",
    "user_websites.blog_post_edit_form",
    "user_websites.community_directory",
    "user_websites.documentation_page",
    "user_websites.owner_delete_confirm",
    "user_websites.owner_page_edit",
    "user_websites.owner_site_dashboard",
    "user_websites.owner_site_ineligible",
    "user_websites.owner_site_suspended",
    "user_websites.placeholder_page",
    "user_websites.privacy_erased",
    "user_websites.report_violation_page",
    "user_websites.unsubscribe_success",
]


@tagged("post_install", "-at_install")
class TestLayoutSeo(HamsTransactionCase):
    def test_pages_have_title_and_description(self):
        assert_layout_seo(self, LAYOUT_TEMPLATES)
