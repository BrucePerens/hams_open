# -*- coding: utf-8 -*-
import logging
from odoo.tests import tagged
from odoo.addons.zero_sudo.tests.common import HamsHttpCase

_logger = logging.getLogger(__name__)


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
        # We do NOT call self.env.cr.commit() here.
        # HamsHttpCase safely handles the transaction and automatically rolls it back after the test.

    def test_01_seo_widget_tour(self):
        # [@ANCHOR: test_seo_widget_tour]
        # Verified by [@ANCHOR: test_seo_widget_tour]
        """Execute the SEO Optimization UI Tour as the admin user."""
        # The admin user logs in to the backend to configure the portal user's SEO data
        self.start_tour("/odoo?debug=1", "user_websites_seo_tour", login="admin")
