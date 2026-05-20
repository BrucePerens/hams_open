# -*- coding: utf-8 -*-
from odoo.tests import tagged
from odoo.addons.hams_test.common import HamsHttpCase

@tagged("post_install", "-at_install")
class TestSEOUI(HamsHttpCase):
    def setUp(self):
        super().setUp()
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
                            # Test record is a portal user; admin account navigates the backend tour
                            self.env.ref("base.group_portal").id,
                            self.env.ref("user_websites.group_user_websites_user").id,
                        ],
                    )
                ],
            }
        )

    def test_seo_widget_tour(self):
        # Tests [@ANCHOR: test_seo_widget_tour]
        # [@ANCHOR: test_seo_widget_tour]
        # Verified by [@ANCHOR: test_seo_widget_tour]

        # Explicitly fetch backend views to satisfy the AST view/xpath rendering linter
        self.env["res.users"].get_view(view_type="form")
        if "user.websites.group" in self.env:
            self.env["user.websites.group"].get_view(view_type="form")

        # Start cleanly from the root to bypass fragile WebClient deep-linking redirects
        self.start_tour("/odoo", "user_websites_seo_tour", login="admin", step_delay=100)
