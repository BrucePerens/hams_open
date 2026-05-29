# -*- coding: utf-8 -*-
import logging
from odoo.tests import tagged
from odoo.addons.zero_sudo.tests.real_transaction import RealTransactionCase

_logger = logging.getLogger(__name__)


@tagged("post_install", "-at_install")
class TestSEOUI(RealTransactionCase):
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
        # Enforce commit to ensure test data is visible to separate HTTP worker threads
        self.env.cr.commit()

    def tearDown(self):
        # Explicit resilient cleanup to prevent database pollution
        for attempt in range(5):
            try:
                with self.env.cr.savepoint():
                    if getattr(self, 'user_test', False) and self.user_test.exists():
                        self.user_test.unlink()
                break
            except Exception as e: # audit-ignore-catch-all
                _logger.warning("Resilient cleanup encountered exception: %s", e)

        self.env.cr.commit()
        super().tearDown()

    def test_01_seo_widget_tour(self):
        """Execute the SEO Optimization UI Tour as the portal user."""
        self.start_tour("/blog?debug=1", "user_websites_seo_tour", login="seouitest")
