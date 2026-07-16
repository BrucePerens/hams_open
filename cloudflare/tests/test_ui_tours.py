# This software is distributed under the terms of the Affero General Public License (AGPL-3).

# -*- coding: utf-8 -*-
from odoo.tests.common import tagged
from odoo.addons.zero_sudo.tests.common import HamsHttpCase


@tagged("post_install", "-at_install")
class TestCloudflareUITours(HamsHttpCase):

    def setUp(self):
        super().setUp()

        # Seed backend records to be asserted by the frontend tours
        self.env["cloudflare.ip.ban"].create(
            {
                "ip_address": "192.168.9.9",
                "mode": "block",
                "state": "active",
                "notes": "Tour Seed Record",
            }
        )

        self.env["cloudflare.waf.rule"].create(
            {
                "name": "Tour XML-RPC Rule",
                "action": "block",
                "expression": 'http.request.uri.path contains "/tour"',
            }
        )

        # Ensure the admin has system access to view these menus
        self.admin = self.env.ref("base.user_admin")
        self.admin.lang = "en_US"

    def test_01_ip_ban_tour(self):
        # Tests [@ANCHOR: COMM_cf_ip_ban_tour]
        """Executes the JS tour simulating an administrator reviewing honeypot bans."""
        self.authenticate(self.admin.login, self.admin.login)
        self.start_tour("/odoo?debug=1", "cf_ip_ban_tour", login=self.admin.login)

    def test_02_waf_rule_tour(self):
        # Tests [@ANCHOR: COMM_cf_waf_rule_tour]
        """Executes the JS tour simulating an administrator viewing WAF Edge configurations."""
        self.authenticate(self.admin.login, self.admin.login)
        self.start_tour("/odoo?debug=1", "cf_waf_rule_tour", login=self.admin.login)

    def test_03_purge_wizard_tour(self):
        # Tests [@ANCHOR: COMM_cf_purge_wizard_tour]
        """Executes the JS tour for the Manual Cache Purge Wizard."""
        mock_creds = self.safe_patch("odoo.addons.cloudflare.models.website.WebsiteCloudflare._get_cloudflare_credentials")
        mock_creds.return_value = ("fake_token", "fake_zone")

        mock_purge = self.safe_patch(
            "odoo.addons.cloudflare.models.purge_wizard.purge_everything"
        )
        mock_purge.return_value = True

        self.authenticate(self.admin.login, self.admin.login)
        self.start_tour("/odoo?debug=1", "cf_purge_wizard_tour", login=self.admin.login)

    def test_04_zone_settings_tour(self):
        # Tests [@ANCHOR: COMM_cf_zone_settings_tour]
        """Executes the JS tour for the Zone Settings Wizard."""
        mock_creds = self.safe_patch("odoo.addons.cloudflare.models.website.WebsiteCloudflare._get_cloudflare_credentials")
        mock_creds.return_value = ("fake_token", "fake_zone")

        # Mock the API calls in the wizard's default_get and action_apply_settings
        mock_get = self.safe_patch(
            "odoo.addons.cloudflare.models.zone_settings_wizard.get_zone_settings"
        )
        mock_get.return_value = []
        
        mock_update = self.safe_patch(
            "odoo.addons.cloudflare.models.zone_settings_wizard.update_zone_setting"
        )
        mock_update.return_value = (True, "Success")

        self.authenticate(self.admin.login, self.admin.login)
        self.start_tour(
            "/odoo?debug=1", "cf_zone_settings_tour", login=self.admin.login
        )

    def test_05_backend_views_rendering(self):
        # Tests [@ANCHOR: COMM_cf_backend_views_rendering]
        v1 = self.env["cloudflare.config.backup"].get_view(view_type="list")
        self.assertIn("create_date", v1["arch"])

        v2 = self.env["cloudflare.ip.ban"].get_view(view_type="list")
        self.assertIn("ip_address", v2["arch"])

        v3 = self.env["cloudflare.tunnel.wizard"].get_view(view_type="form")
        self.assertIn("command", v3["arch"])

        v4 = self.env["cloudflare.waf.rule"].get_view(view_type="list")
        self.assertIn("action", v4["arch"])

        v5 = self.env["cloudflare.dns.record"].get_view(view_type="list")
        self.assertIn("content", v5["arch"])

        v6 = self.env["cloudflare.zone.settings"].get_view(view_type="form")
        self.assertIn("ssl_mode", v6["arch"])

        v7 = self.env["cloudflare.rate.limit"].get_view(view_type="list")
        self.assertIn("mitigation_action", v7["arch"])

        v8 = self.env["cloudflare.cache.rule"].get_view(view_type="list")
        self.assertIn("edge_cache_ttl", v8["arch"])

        v9 = self.env["cloudflare.zero.trust.policy"].get_view(view_type="form")
        self.assertIn("policy_action", v9["arch"])
