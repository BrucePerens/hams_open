# Copyright © Bruce Perens K6BP.
# SPDX-License-Identifier: AGPL-3.0-or-later

# -*- coding: utf-8 -*-
"""A tunnel (or website) with no Cloudflare API token or account id is refused locally, before any Cloudflare call:
push configuration raises, delete reports it and keeps the record, sync does nothing, and the edge-state
initialisation skips a website with no credentials instead of calling the API with empty ones. Every Cloudflare
function is replaced; nothing leaves the process."""
from cryptography.fernet import Fernet
from odoo.exceptions import UserError
from odoo.tests.common import tagged
from odoo.addons.zero_sudo.tests.common import HamsTransactionCase

TUNNEL = "odoo.addons.cloudflare.models.tunnel."
MANAGER = "odoo.addons.cloudflare.models.config_manager."


@tagged("post_install", "-at_install")
class TestTunnelCredentialRefusals(HamsTransactionCase):
    def setUp(self):
        super().setUp()
        self.Tunnel = self.env["cloudflare.tunnel"]
        self.safe_patch(
            "odoo.addons.cloudflare.models.website.WebsiteCloudflare._get_fernet"
        ).return_value = Fernet(Fernet.generate_key())

    def _tunnel(self, slug, with_credentials):
        website = self.env["website"].create(
            {"name": "Refusal %s" % slug, "domain": "https://%s.example.com" % slug}
        )
        if with_credentials:
            website.write(
                {
                    "cloudflare_api_token": "tok-%s" % slug,
                    "cloudflare_zone_id": "zone-%s" % slug,
                    "cloudflare_account_id": "acct-%s" % slug,
                }
            )
        return self.Tunnel.create(
            {"cf_tunnel_id": "cftun-%s" % slug, "name": "Tunnel %s" % slug, "website_id": website.id}
        )

    def test_01_push_configuration_without_credentials_raises_and_calls_nothing(self):
        # Tests [@ANCHOR: cloudflare:COMM_tunnel_action_push_configuration]
        update = self.safe_patch(TUNNEL + "update_cfd_tunnel_configuration")
        tunnel = self._tunnel("pushnone", False)
        with self.assertRaises(UserError) as caught:
            tunnel.action_push_configuration()
        self.assertIn("Missing Cloudflare API Token or Account ID", str(caught.exception))
        update.assert_not_called()

    def test_02_delete_without_credentials_keeps_the_record_and_calls_nothing(self):
        # Tests [@ANCHOR: COMM_cf_delete_tunnel]
        delete = self.safe_patch(TUNNEL + "delete_cfd_tunnel")
        self.safe_patch(TUNNEL + "stop_tunnel_daemon")
        tunnel = self._tunnel("delnone", False)
        with self.assertRaises(UserError) as caught:
            tunnel.action_delete_tunnel()
        self.assertIn("missing Cloudflare API Token or Account ID", str(caught.exception))
        delete.assert_not_called()
        self.assertTrue(tunnel.exists())

    def test_03_a_mixed_delete_removes_the_good_one_and_warns_about_the_other(self):
        # Tests [@ANCHOR: COMM_cf_delete_tunnel]
        delete = self.safe_patch(TUNNEL + "delete_cfd_tunnel")
        delete.return_value = (True, "")
        self.safe_patch(TUNNEL + "stop_tunnel_daemon")
        good = self._tunnel("delgood", True)
        bad = self._tunnel("delbad", False)
        result = (good | bad).action_delete_tunnel()
        self.assertEqual(result["params"]["type"], "warning")
        self.assertIn("missing Cloudflare API Token or Account ID", result["params"]["message"])
        delete.assert_called_once()
        self.assertEqual(delete.call_args.args[2], "cftun-delgood")
        self.assertFalse(good.exists())
        self.assertTrue(bad.exists())

    def test_04_sync_for_a_website_without_credentials_or_a_missing_website_calls_nothing(self):
        # Tests [@ANCHOR: cloudflare:COMM_sync_tunnels_for_website]
        listing = self.safe_patch(TUNNEL + "list_cfd_tunnels")
        tunnel = self._tunnel("syncnone", False)
        self.Tunnel._sync_tunnels_for_website(tunnel.website_id.id)
        self.Tunnel._sync_tunnels_for_website(2147483000)
        listing.assert_not_called()

    def test_05_edge_state_initialisation_skips_a_website_with_no_credentials(self):
        # Tests [@ANCHOR: cloudflare:COMM_initialize_cloudflare_state]
        self._tunnel("edgenone", False)
        get_ruleset = self.safe_patch(MANAGER + "get_zone_ruleset_checked")
        get_ruleset.return_value = ("absent", None)
        for name in ("create_zone_ruleset", "update_zone_ruleset"):
            self.safe_patch(MANAGER + name)
        self.env["cloudflare.config.manager"].initialize_cloudflare_state()
        for call in get_ruleset.call_args_list:
            self.assertTrue(call.args[1], "a Cloudflare call was made with an empty token")
            self.assertTrue(call.args[2], "a Cloudflare call was made with an empty zone id")

    def test_06_edge_state_initialisation_leaves_a_zone_it_cannot_read_alone(self):
        # Tests [@ANCHOR: cloudflare:COMM_initialize_cloudflare_state]
        tunnel = self._tunnel("edgeerr", True)
        website = tunnel.website_id
        self.safe_patch(MANAGER + "get_zone_ruleset").return_value = None  # what an outage looked like before
        self.safe_patch(MANAGER + "get_zone_ruleset_checked").return_value = ("error", "HTTP 500")
        push = self.safe_patch_object(type(self.env["cloudflare.config.manager"]), "action_push_waf_rules")
        self.env["cloudflare.config.manager"].initialize_cloudflare_state()
        push.assert_not_called()
        self.assertFalse(self.env["cloudflare.waf.rule"].search_count([("website_id", "=", website.id)]))
