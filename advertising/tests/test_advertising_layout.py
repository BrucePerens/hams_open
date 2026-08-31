# -*- coding: utf-8 -*-
# Copyright © HAMS project. AGPL-3.0-or-later.
from odoo.addons.zero_sudo.tests.common import HamsHttpCase
from odoo.tests.common import tagged


@tagged("post_install", "-at_install")
class TestAdvertisingLayout(HamsHttpCase):
    def setUp(self):
        super().setUp()
        self.website = self.env["website"].get_current_website()
        self.website.google_adsense_client_id = False
        self.website.google_adsense_footer_slot_id = False
        # A real, self-contained /shack-prefixed page, created here rather
        # than relying on ham_shack's own real /shack route: ham_shack lives
        # in a separate repo (hams_com) this module has no dependency on, so
        # an isolated hams_open-only test run has no real /shack route to
        # hit. Extending website.layout directly (the same shape any real
        # website.page's arch_db uses) is what actually exercises this
        # template's own path-based exclusion, independent of ham_shack.
        shack_view = self.env["ir.ui.view"].create(
            {
                "name": "Advertising Test Shack Page",
                "type": "qweb",
                "key": "advertising.test_shack_page",
                "arch_db": (
                    "<t name=\"Advertising Test Shack Page\" "
                    "t-name=\"advertising.test_shack_page\">"
                    "<t t-call=\"website.layout\">"
                    "<div>shack test content</div>"
                    "</t></t>"
                ),
            }
        )
        self.env["website.page"].create(
            {
                "is_published": True,
                "url": "/shack",
                "website_id": self.website.id,
                "view_id": shack_view.id,
            }
        )

    def test_01_no_ad_markup_when_unconfigured(self):
        # [@ANCHOR: test_xpath_rendering_advertising]
        response = self.url_open("/")
        self.assertEqual(response.status_code, 200)
        self.assertNotIn(
            "adsbygoogle",
            response.text,
            "[!] DIAGNOSTIC FOR AI: the default (unconfigured) state must render "
            "zero ad-related markup or script -- found adsbygoogle anyway.",
        )
        self.assertNotIn("adsense_loader", response.text)
        self.assertNotIn("advertising_footer_slot", response.text)

    def test_02_client_id_alone_loads_script_but_no_footer_slot(self):
        # Tests [@ANCHOR: xpath_rendering_advertising_head]
        self.website.google_adsense_client_id = "ca-pub-1234567890123456"
        response = self.url_open("/")
        self.assertEqual(response.status_code, 200)
        self.assertIn(
            "adsense_loader",
            response.text,
            "[!] DIAGNOSTIC FOR AI: the loader script must render once a "
            "publisher ID is configured, independent of the footer slot ID.",
        )
        self.assertIn("ca-pub-1234567890123456", response.text)
        self.assertNotIn(
            "advertising_footer_slot",
            response.text,
            "[!] DIAGNOSTIC FOR AI: the footer ad slot must NOT render until "
            "BOTH the publisher ID and the footer slot ID are configured.",
        )

    def test_03_both_configured_renders_the_footer_slot(self):
        # Tests [@ANCHOR: xpath_rendering_advertising_footer]
        self.website.google_adsense_client_id = "ca-pub-1234567890123456"
        self.website.google_adsense_footer_slot_id = "9876543210"
        response = self.url_open("/")
        self.assertEqual(response.status_code, 200)
        self.assertIn("advertising_footer_slot", response.text)
        self.assertIn('data-ad-client="ca-pub-1234567890123456"', response.text)
        self.assertIn('data-ad-slot="9876543210"', response.text)

    def test_04_shack_console_never_shows_the_footer_ad(self):
        # Tests [@ANCHOR: xpath_rendering_advertising_footer]
        # /shack is a real-time operating console, explicitly excluded per
        # this proposal's own placement-density guidance. Confirmed against
        # the self-contained /shack page created in setUp AND, as a negative
        # control, that the same fully-configured state still shows the ad
        # on an ordinary page -- proving the exclusion is real and specific
        # to the /shack prefix, not just "this template never renders."
        self.website.google_adsense_client_id = "ca-pub-1234567890123456"
        self.website.google_adsense_footer_slot_id = "9876543210"
        shack_response = self.url_open("/shack")
        self.assertEqual(shack_response.status_code, 200)
        self.assertNotIn(
            "advertising_footer_slot",
            shack_response.text,
            "[!] DIAGNOSTIC FOR AI: /shack must never carry the footer ad slot, "
            "even when fully configured -- it's a real-time console, not a "
            "content/reference page.",
        )
        home_response = self.url_open("/")
        self.assertIn("advertising_footer_slot", home_response.text)

    def test_05_consent_mode_default_denies_until_accepted(self):
        # Tests [@ANCHOR: xpath_rendering_advertising_head]
        # Confirms this template's own consent wiring, independent of
        # whether Google Analytics is also configured on this site --
        # google_analytics_key stays empty throughout this test. The cookies
        # bar must be enabled for _allConsentsGranted() to actually gate on
        # the event rather than short-circuit to "already granted" -- its
        # own docstring says so, confirmed by reading it directly rather
        # than assumed.
        self.assertFalse(self.website.google_analytics_key)
        self.website.cookies_bar = True
        self.website.google_adsense_client_id = "ca-pub-1234567890123456"
        response = self.url_open("/")
        self.assertIn("'ad_storage': 'denied'", response.text)
        self.assertIn("optionalCookiesAccepted", response.text)

    def test_06_xpath_rendering_settings(self):
        # [@ANCHOR: test_xpath_rendering_advertising_settings]
        """Verify the Advertising settings block is injected into the
        compiled website configuration view."""
        # Tests [@ANCHOR: xpath_rendering_advertising_settings]
        view = self.env.ref("website.res_config_settings_view_form")
        arch_str = self.env["res.config.settings"].with_context(lang=None).get_view(
            view_id=view.id, view_type="form"
        )["arch"]
        self.assertIn(
            "google_adsense_client_id",
            arch_str,
            "The Advertising settings block must be injected into the "
            "compiled website settings view.",
        )
