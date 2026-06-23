# -*- coding: utf-8 -*-
import logging
from odoo.tests import tagged
from odoo.addons.zero_sudo.tests.real_transaction import RealTransactionCase

_logger = logging.getLogger(__name__)


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
        self.env.ref("base.user_admin").lang = "en_US"
        self.env.cr.commit()

    def test_01_seo_widget_tour(self):
        # [@ANCHOR: test_seo_widget_tour]

        """Execute the SEO Optimization UI Tour as the admin user."""
        self.start_tour(
            "/odoo?debug=1",
            "user_websites_seo_tour",
            login="admin"
        )
