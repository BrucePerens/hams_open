# -*- coding: utf-8 -*-
# Copyright © Bruce Perens K6BP. Licensed under the GNU Affero General Public License v3.0 (AGPL-3.0).
from odoo.tests.common import tagged
from odoo.addons.zero_sudo.tests.common import HamsTransactionCase
from odoo.addons.compliance.hooks import post_init_hook


@tagged("post_install", "-at_install")
class TestComplianceHooks(HamsTransactionCase):

    def test_02_post_init_hook_cookie_bar(self):
        """
        Verify that the post_init_hook successfully enables the cookies_bar
        on all websites.
        """
        # [@ANCHOR: test_compliance_post_init_cookie_bar]
        # Tests [@ANCHOR: compliance_post_init_cookie_bar]
        # Tests [@ANCHOR: story_cookie_consent]
        # Tests [@ANCHOR: journey_compliance_setup]

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
            "is_published": True,
            "website_id": False
        })

        # Ensure our boilerplate page exists and is published (default state)
        boilerplate_page = self.env.ref("compliance.page_privacy_policy")
        boilerplate_page.write({"is_published": True, "website_id": False})

        self.env.flush_all()
        # Run the hook
        post_init_hook(self.env)
        self.env['website.page'].invalidate_model(['is_published'])

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

        # Multi-Website awareness test
        website_2 = self.env["website"].create({"name": "Test Website 2"})
        # Create a custom page ONLY for website_2
        custom_view_2 = self.env["ir.ui.view"].create({
            "name": "Custom Cookie 2",
            "type": "qweb",
            "arch": "<div>Custom Cookie 2</div>",
            "key": "custom.cookie_view_2"
        })
        custom_page_2 = self.env["website.page"].create({
            "url": "/cookie-policy",
            "view_id": custom_view_2.id,
            "is_published": True,
            "website_id": website_2.id
        })

        # Ensure boilerplate is published
        boilerplate_cookie = self.env.ref("compliance.page_cookie_policy")
        boilerplate_cookie.write({"is_published": True, "website_id": False})

        # Pre-cleanup: unpublish any existing boilerplate for website 2
        existing_bp_2 = self.env["website.page"].with_context(active_test=False).search([
            ("url", "=", "/cookie-policy"),
            ("website_id", "=", website_2.id)
        ]).filtered(lambda p: p.view_id.key and p.view_id.key.startswith("compliance.compliance_"))
        existing_bp_2.write({"is_published": False})

        self.env.flush_all()
        post_init_hook(self.env)
        boilerplate_cookie.invalidate_recordset(['is_published'])

        # Global boilerplate should NOT be unpublished by a website-specific custom page
        self.assertTrue(
            boilerplate_cookie.is_published,
            "Global boilerplate should NOT be unpublished by a website-specific custom page."
        )

        # Cleanup
        custom_page.unlink()
        custom_view.unlink()
        custom_page_2.unlink()
        custom_view_2.unlink()
        website_2.unlink()

    def test_05_website_default_cookie_bar(self):
        """Verify that new websites have cookies_bar enabled by default."""
        # AI Laziness Fix Test: Ensure our model inheritance works.
        new_website = self.env["website"].create({"name": "New Compliant Website"})
        self.assertTrue(
            new_website.cookies_bar,
            "New websites should have cookies_bar enabled by default for compliance."
        )
        new_website.unlink()

    def test_06_boilerplate_restoration(self):
        """Verify that boilerplate is restored if custom page is removed."""
        # Create a custom page
        custom_view = self.env["ir.ui.view"].create({
            "name": "Temp Custom Privacy",
            "type": "qweb",
            "arch": "<div>Custom Content</div>",
            "key": "custom.temp_privacy_view"
        })
        custom_page = self.env["website.page"].create({
            "url": "/privacy",
            "view_id": custom_view.id,
            "is_published": True,
            "website_id": False
        })

        # Run hook to unpublish boilerplate
        post_init_hook(self.env)
        boilerplate_page = self.env.ref("compliance.page_privacy_policy")
        self.assertFalse(boilerplate_page.is_published)

        # Remove custom page
        custom_page.unlink()
        custom_view.unlink()

        # Run hook again
        post_init_hook(self.env)
        self.assertTrue(boilerplate_page.is_published, "Boilerplate should be re-published if custom page is gone.")
