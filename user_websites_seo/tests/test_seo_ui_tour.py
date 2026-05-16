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
                            # MUST be an Internal User to access /web and view backend res.users form
                            self.env.ref("base.group_user").id,
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

        # Start the tour with the authenticated context on the backend URL
        self.start_tour("/web", "user_websites_seo_tour", login=self.user_test.login)
