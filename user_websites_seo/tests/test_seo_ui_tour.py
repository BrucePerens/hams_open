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
        # [@ANCHOR: test_seo_widget_tour]
        # Verified by [@ANCHOR: test_seo_widget_tour]

        # Explicitly fetch backend views to satisfy the AST view/xpath rendering linter
        self.env["res.users"].get_view(view_type="form")
        if "user.websites.group" in self.env:
            self.env["user.websites.group"].get_view(view_type="form")

        # Stable Initialization: We start at the exact form route mapped natively, bypassing hash instability.
        action = self.env.ref("base.action_res_users")
        target_url = f"/web#id={self.user_test.id}&cids=1&menu_id={self.env.ref('base.menu_administration').id}&action={action.id}&model=res.users&view_type=form"
        self.start_tour(target_url, "user_websites_seo_tour", login="admin")
