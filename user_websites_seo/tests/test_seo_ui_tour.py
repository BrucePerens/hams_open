# This software is distributed under the terms of the Affero General Public License (AGPL-3).

# -*- coding: utf-8 -*-
from odoo.tests import tagged
from odoo.addons.zero_sudo.tests.real_transaction import RealTransactionCase


@tagged("post_install", "-at_install")
class TestSEOUI(RealTransactionCase):
    def setUp(self):
        super().setUp()
        group_user_websites_user = self.env.ref(
            "user_websites.group_user_websites_user"
        ).id
        self.user_test = self.env["res.users"].create(
            {
                "name": "SEO UI Test User",
                "login": "seouitest",
                "password": "seouitest",
                "website_slug": "seo-ui-test-user",
                "lang": "en_US",
                "group_ids": [
                    (
                        6,
                        0,
                        [
                            self.env.ref("base.group_portal").id,
                            group_user_websites_user,
                        ],
                    )
                ],
            }
        )
        self.env.cr.commit()

    def test_01_seo_widget_tour(self):
        # Tests [@ANCHOR: zero_sudo:patched_browser_js]
        # (RealTransactionCase extends HttpCase directly, not
        # HamsHttpCase, so it has no own browser_js() override -- calling
        # start_tour() here falls through to core Odoo's HttpCase.
        # start_tour(), which calls self.browser_js(), which resolves to
        # the module-level monkeypatch zero_sudo/tests/common.py applies
        # directly to HttpCase.browser_js at import time.)
        # [@ANCHOR: COMM_test_seo_widget_tour]
        """Execute the SEO Optimization UI Tour as the admin user to edit the portal user."""
        self.start_tour("/odoo?debug=1", "user_websites_seo_tour", login="admin")
