# -*- coding: utf-8 -*-
# Copyright © Bruce Perens K6BP. Licensed under the GNU Affero General Public License v3.0 (AGPL-3.0).
import unittest
from odoo.tests.common import TransactionCase, tagged
from odoo.addons.compliance.hooks import post_init_hook


@tagged("post_install", "-at_install")
class TestComplianceHooks(TransactionCase):

    def test_02_post_init_hook_cookie_bar(self):
        """
        Verify that the post_init_hook successfully enables the cookies_bar
        on all websites.
        """
        # [@ANCHOR: test_compliance_post_init_cookie_bar]
        # Tests [@ANCHOR: compliance_post_init_cookie_bar]
        # Tests [@ANCHOR: story_cookie_consent]
        # Tests [@ANCHOR: journey_compliance_setup]
        if "cookies_bar" not in self.env["website"]._fields:
            raise unittest.SkipTest(
                "'cookies_bar' field is not present on the website model. Skipping cookie bar hook test."
            )

        post_init_hook(self.env)

        websites = self.env["website"].search([])
        for website in websites:
            self.assertTrue(
                website.cookies_bar,
                f"Cookie bar must be enabled on website: {website.name}",
            )

    def test_03_views_rendering(self):
        """Verify that legal templates render correctly."""
        # [@ANCHOR: test_compliance_views]
        # Tests [@ANCHOR: compliance_legal_pages_rendering]
        # Tests [@ANCHOR: story_automatic_legal_pages]
        # Tests [@ANCHOR: journey_compliance_setup]
        self.env.ref("compliance.compliance_privacy_policy_template").with_context(
            lang=None
        )._get_combined_arch()
        self.env.ref("compliance.compliance_cookie_policy_template").with_context(
            lang=None
        )._get_combined_arch()
        self.env.ref("compliance.compliance_terms_of_service_template").with_context(
            lang=None
        )._get_combined_arch()

    def test_04_non_destructive_mandate(self):
        """
        Verify that if a custom page already exists, the boilerplate is unpublished.
        """
        # [@ANCHOR: test_compliance_non_destructive_mandate]
        # Tests [@ANCHOR: story_automatic_legal_pages]

        # Create a "custom" page at /privacy
        custom_view = self.env["ir.ui.view"].create({
            "name": "Custom Privacy",
            "type": "qweb",
            "arch": "<div>Custom Content</div>",
            "key": "custom.privacy_view"
        })
        custom_page = self.env["website.page"].create({
            "url": "/privacy",
            "view_id": custom_view.id,
            "is_published": True
        })

        # Ensure our boilerplate page exists and is published (default state)
        boilerplate_page = self.env.ref("compliance.page_privacy_policy")
        boilerplate_page.write({"is_published": True})

        # Run the hook
        post_init_hook(self.env)

        # Check that the boilerplate is now unpublished
        self.assertFalse(
            boilerplate_page.is_published,
            "Boilerplate page should be unpublished when a custom page exists at the same URL."
        )
        # Check that the custom page is still published
        self.assertTrue(
            custom_page.is_published,
            "Custom page should remain published."
        )

        # Cleanup
        custom_page.unlink()
        custom_view.unlink()
