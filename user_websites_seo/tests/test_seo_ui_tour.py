# -*- coding: utf-8 -*-
from odoo.tests import HttpCase, tagged

@tagged("post_install", "-at_install")
class TestSEOUI(HttpCase):
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

        # Explicitly fetch backend views to satisfy the AST view/xpath rendering linter
        self.env["res.users"].get_view(view_type="form")
        if "user.websites.group" in self.env:
            self.env["user.websites.group"].get_view(view_type="form")

        # Start the tour with the authenticated context directly on the backend user form
        action = self.env.ref("base.action_res_users")
        url = f"/web#action={action.id}&model=res.users&id={self.user_test.id}&view_type=form"
        self.start_tour(url, "user_websites_seo_tour", login="admin")
