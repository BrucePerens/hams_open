# -*- coding: utf-8 -*-
from odoo.tests.common import tagged
from odoo.addons.hams_test.common import HamsHttpCase


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

    def test_01_ip_ban_tour(self):
        """Executes the JS tour simulating an administrator reviewing honeypot bans."""
        self.authenticate(self.admin.login, self.admin.login)
        self.start_tour("/odoo?debug=1", "cf_ip_ban_tour", login=self.admin.login)

    def test_02_waf_rule_tour(self):
        """Executes the JS tour simulating an administrator viewing WAF Edge configurations."""
        self.authenticate(self.admin.login, self.admin.login)
        self.start_tour("/odoo?debug=1", "cf_waf_rule_tour", login=self.admin.login)

    def test_03_purge_wizard_tour(self):
        """Executes the JS tour for the Manual Cache Purge Wizard."""
        # Seed credentials so the wizard doesn't crash
        website = self.env["website"].get_current_website()
        website.write({
            "cloudflare_api_token": "fake_token",
            "cloudflare_zone_id": "fake_zone"
        })

        mock_purge = self.safe_patch("odoo.addons.cloudflare.models.purge_wizard.purge_everything")
        mock_purge.return_value = True

        self.authenticate(self.admin.login, self.admin.login)
        self.start_tour("/odoo?debug=1", "cf_purge_wizard_tour", login=self.admin.login)

    def test_04_backend_views_rendering(self):
        # [@ANCHOR: test_cf_backend_views_rendering]
        v1 = self.env["cloudflare.config.backup"].get_view(view_type="list")
        self.assertIn("create_date", v1["arch"])

        v2 = self.env["cloudflare.ip.ban"].get_view(view_type="list")
        self.assertIn("ip_address", v2["arch"])

        v3 = self.env["cloudflare.tunnel.wizard"].get_view(view_type="form")
        self.assertIn("command", v3["arch"])

        v4 = self.env["cloudflare.waf.rule"].get_view(view_type="list")
        self.assertIn("action", v4["arch"])
