# This software is distributed under the terms of the Affero General Public License (AGPL-3).

# -*- coding: utf-8 -*-
import requests
from unittest.mock import MagicMock
from odoo.tools import mute_logger
from odoo.tests.common import tagged
from odoo.addons.zero_sudo.tests.common import HamsTransactionCase
from odoo.addons.cloudflare.utils.cloudflare_api import purge_urls, purge_tags


@tagged("post_install", "-at_install")
class TestCloudflareAPIs(HamsTransactionCase):

    def test_01_waf_ban_ip(self):
        # [@ANCHOR: test_cf_ban_ip_api]

        # Tests [@ANCHOR: COMM_cf_ban_ip_api]

        mock_ban_ip = self.safe_patch("odoo.addons.cloudflare.models.ip_ban.ban_ip")
        mock_ban_ip.return_value = (True, "fake_rule_123")

        website = self.env["website"].get_current_website()
        website.write(
            {"cloudflare_api_token": "fake_token", "cloudflare_zone_id": "fake_zone"}
        )
        
        mock_creds = self.safe_patch("odoo.addons.cloudflare.models.website.WebsiteCloudflare._get_cloudflare_credentials")
        mock_creds.return_value = ("fake_token", "fake_zone")

        res = self.env["cloudflare.waf"].ban_ip("192.168.1.100", website_id=website.id)
        self.assertTrue(res)

    def test_02_turnstile_secret_fetch(self):
        # [@ANCHOR: COMM_test_cf_turnstile_verify]

        # Tests [@ANCHOR: COMM_cf_turnstile_verify]

        website = self.env["website"].get_current_website()
        website.write({"cloudflare_turnstile_secret": "my_super_secret_key"})

        mock_post = self.safe_patch(
            "odoo.addons.cloudflare.utils.cloudflare_api.session.post"
        )
        mock_response = MagicMock()
        mock_response.json.return_value = {"success": True}
        mock_post.return_value = mock_response

        res = self.env["cloudflare.turnstile"].verify_token(
            "fake_token_123", "odoo", website_id=website.id
        )
        self.assertTrue(res)

        called_data = mock_post.call_args[1]["data"]
        self.assertEqual(called_data["secret"], "my_super_secret_key")
        self.assertEqual(called_data["response"], "fake_token_123")

    def test_03_tunnel_setup(self):
        # [@ANCHOR: COMM_test_cf_tunnel_setup]

        # Tests [@ANCHOR: COMM_cf_tunnel_setup]
        mock_create = self.safe_patch(
            "odoo.addons.cloudflare.models.res_config_settings.create_cfd_tunnel"
        )
        mock_get_token = self.safe_patch(
            "odoo.addons.cloudflare.models.res_config_settings.get_cfd_tunnel_token"
        )

        mock_create.return_value = (True, "tunnel_id_123")
        mock_get_token.return_value = (True, "mock_token_xyz")

        website = self.env["website"].get_current_website()
        website.write(
            {"cloudflare_account_id": "acc123", "cloudflare_api_token": "tok123"}
        )
        settings = self.env["res.config.settings"].create({"website_id": website.id})

        action = settings.action_generate_tunnel_command()
        self.assertEqual(action["res_model"], "cloudflare.tunnel.wizard")

        wizard = self.env["cloudflare.tunnel.wizard"].browse(action["res_id"])
        self.assertIn("mock_token_xyz", wizard.command)

    def test_04_sync_tunnels(self):
        # [@ANCHOR: COMM_test_cf_sync_tunnels]

        # Tests [@ANCHOR: COMM_cf_sync_tunnels]
        mock_list = self.safe_patch(
            "odoo.addons.cloudflare.models.tunnel.list_cfd_tunnels"
        )
        mock_list.return_value = [
            {
                "id": "t1",
                "name": "Tunnel 1",
                "status": "healthy",
                "created_at": "2021-01-01T00:00:00Z",
            }
        ]
        website = self.env["website"].get_current_website()
        website.write(
            {"cloudflare_account_id": "acc123", "cloudflare_api_token": "tok123"}
        )

        self.env["cloudflare.tunnel"].action_sync_tunnels()
        tunnel = self.env["cloudflare.tunnel"].search(
            [("cf_tunnel_id", "=", "t1")], limit=1
        )
        self.assertTrue(tunnel)
        self.assertEqual(tunnel.name, "Tunnel 1")

    def test_05_delete_tunnel(self):
        # [@ANCHOR: COMM_test_cf_delete_tunnel]

        # Tests [@ANCHOR: COMM_cf_delete_tunnel]
        mock_delete = self.safe_patch(
            "odoo.addons.cloudflare.models.tunnel.delete_cfd_tunnel"
        )
        mock_delete.return_value = (True, "Success")
        website = self.env["website"].get_current_website()
        website.write(
            {"cloudflare_account_id": "acc123", "cloudflare_api_token": "tok123"}
        )
        tunnel = self.env["cloudflare.tunnel"].create(
            {"cf_tunnel_id": "t1", "name": "Tunnel 1", "website_id": website.id}
        )
        tunnel.action_delete_tunnel()
        self.assertFalse(tunnel.exists())

    def test_06_purge_urls(self):
        # [@ANCHOR: COMM_test_purge_urls_api]

        # Tests [@ANCHOR: COMM_cf_purge_urls_api]

        mock_post = self.safe_patch(
            "odoo.addons.cloudflare.utils.cloudflare_api.session.post"
        )

        # Case 1: Missing credentials
        self.assertFalse(purge_urls(["https://a.com"], None, "zone1"))
        self.assertFalse(purge_urls(["https://a.com"], "tok1", None))

        # Case 2: Empty URLs
        self.assertTrue(purge_urls([], "tok1", "zone1"))
        mock_post.assert_not_called()

        # Case 3: Success path
        mock_response = MagicMock()
        mock_response.status_code = 200
        mock_response.raise_for_status = MagicMock()
        mock_post.return_value = mock_response

        urls = ["https://a.com/1", "https://a.com/2"]
        res = purge_urls(urls, "fake_token", "fake_zone")
        self.assertTrue(res)

        mock_post.assert_called_once()
        _, kwargs = mock_post.call_args
        self.assertEqual(kwargs["json"]["files"], urls)
        self.assertEqual(kwargs["headers"]["Authorization"], "Bearer fake_token")

        # Case 4: Batching (Chunking to max 30)
        mock_post.reset_mock()
        many_urls = [f"https://a.com/{i}" for i in range(40)]
        purge_urls(many_urls, "fake_token", "fake_zone")
        self.assertEqual(mock_post.call_count, 2)
        self.assertEqual(len(mock_post.call_args_list[0][1]["json"]["files"]), 30)
        self.assertEqual(len(mock_post.call_args_list[1][1]["json"]["files"]), 10)

        # Case 5: API failure
        mock_post.reset_mock()
        mock_post.side_effect = requests.exceptions.RequestException("API fail")

        with mute_logger("odoo.addons.cloudflare.utils.cloudflare_api"):
            self.assertFalse(purge_urls(["https://a.com"], "tok1", "zone1"))

    def test_07_purge_tags(self):
        # [@ANCHOR: COMM_test_purge_tags_api]

        # Tests [@ANCHOR: COMM_cf_purge_tags_api]

        mock_post = self.safe_patch(
            "odoo.addons.cloudflare.utils.cloudflare_api.session.post"
        )

        # Case 1: Missing credentials
        self.assertFalse(purge_tags(["tag1"], None, "zone1"))
        self.assertFalse(purge_tags(["tag1"], "tok1", None))

        # Case 2: Empty tags
        self.assertTrue(purge_tags([], "tok1", "zone1"))
        mock_post.assert_not_called()

        # Case 3: Success path
        mock_response = MagicMock()
        mock_response.status_code = 200
        mock_response.raise_for_status = MagicMock()
        mock_post.return_value = mock_response

        tags = ["tag-a", "tag-b"]
        res = purge_tags(tags, "fake_token", "fake_zone")
        self.assertTrue(res)

        mock_post.assert_called_once()
        _, kwargs = mock_post.call_args
        self.assertEqual(kwargs["json"]["tags"], tags)
        self.assertEqual(kwargs["headers"]["Authorization"], "Bearer fake_token")

        # Case 4: Batching (Chunking to max 30)
        mock_post.reset_mock()
        many_tags = [f"tag-{i}" for i in range(40)]
        purge_tags(many_tags, "fake_token", "fake_zone")
        self.assertEqual(mock_post.call_count, 2)
        self.assertEqual(len(mock_post.call_args_list[0][1]["json"]["tags"]), 30)
        self.assertEqual(len(mock_post.call_args_list[1][1]["json"]["tags"]), 10)

        # Case 5: API failure
        mock_post.reset_mock()
        mock_post.side_effect = requests.exceptions.RequestException("API fail")

        with mute_logger("odoo.addons.cloudflare.utils.cloudflare_api"):
            self.assertFalse(purge_tags(["tag1"], "tok1", "zone1"))
