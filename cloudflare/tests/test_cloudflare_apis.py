# -*- coding: utf-8 -*-
from unittest.mock import patch, MagicMock
from odoo.tests.common import TransactionCase, tagged
from odoo.addons.cloudflare.utils.cloudflare_api import purge_urls, purge_tags


@tagged("post_install", "-at_install")
class TestCloudflareAPIs(TransactionCase):

    @patch("odoo.addons.cloudflare.models.ip_ban.ban_ip")
    def test_01_waf_ban_ip(self, mock_ban_ip):
        # [@ANCHOR: test_cf_ban_ip_api]
        # Tests [@ANCHOR: cf_ban_ip_api]
        # # Verified by [@ANCHOR: test_cf_ban_ip_api]
        mock_ban_ip.return_value = (True, "fake_rule_123")

        website = self.env["website"].get_current_website()
        website.write(
            {"cloudflare_api_token": "fake_token", "cloudflare_zone_id": "fake_zone"}
        )

        res = self.env["cloudflare.waf"].ban_ip("192.168.1.100", website_id=website.id)
        self.assertTrue(res)

    @patch("odoo.addons.cloudflare.utils.cloudflare_api.requests.post")
    def test_02_turnstile_secret_fetch(self, mock_post):
        # [@ANCHOR: test_cf_turnstile_verify]
        # Tests [@ANCHOR: cf_turnstile_verify]
        # # Verified by [@ANCHOR: test_cf_turnstile_verify]
        website = self.env["website"].get_current_website()
        website.write({"cloudflare_turnstile_secret": "my_super_secret_key"})

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

    @patch("odoo.addons.cloudflare.models.res_config_settings.create_cfd_tunnel")
    @patch("odoo.addons.cloudflare.models.res_config_settings.get_cfd_tunnel_token")
    def test_03_tunnel_setup(self, mock_get_token, mock_create):
        # [@ANCHOR: test_cf_tunnel_setup]
        # Tests [@ANCHOR: cf_tunnel_setup]
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

    @patch("odoo.addons.cloudflare.models.tunnel.list_cfd_tunnels")
    def test_04_sync_tunnels(self, mock_list):
        # [@ANCHOR: test_cf_sync_tunnels]
        # Tests [@ANCHOR: cf_sync_tunnels]
        mock_list.return_value = [
            {"id": "t1", "name": "Tunnel 1", "status": "healthy", "created_at": "2021-01-01T00:00:00Z"}
        ]
        website = self.env["website"].get_current_website()
        website.write(
            {"cloudflare_account_id": "acc123", "cloudflare_api_token": "tok123"}
        )

        self.env["cloudflare.tunnel"].action_sync_tunnels()
        tunnel = self.env["cloudflare.tunnel"].search([("cf_tunnel_id", "=", "t1")])
        self.assertTrue(tunnel)
        self.assertEqual(tunnel.name, "Tunnel 1")

    @patch("odoo.addons.cloudflare.models.tunnel.delete_cfd_tunnel")
    def test_05_delete_tunnel(self, mock_delete):
        # [@ANCHOR: test_cf_delete_tunnel]
        # Tests [@ANCHOR: cf_delete_tunnel]
        mock_delete.return_value = (True, "Success")
        website = self.env["website"].get_current_website()
        website.write(
            {"cloudflare_account_id": "acc123", "cloudflare_api_token": "tok123"}
        )
        tunnel = self.env["cloudflare.tunnel"].create({
            "cf_tunnel_id": "t1",
            "name": "Tunnel 1",
            "website_id": website.id
        })
        tunnel.action_delete_tunnel()
        self.assertFalse(tunnel.exists())

    @patch("odoo.addons.cloudflare.utils.cloudflare_api.requests.post")
    def test_04_purge_urls(self, mock_post):
        # [@ANCHOR: test_purge_urls_api]
        # # Verified by [@ANCHOR: test_purge_urls_api]

        # Case 1: Missing credentials
        self.assertFalse(purge_urls(["https://a.com"], None, "zone1"))
        self.assertFalse(purge_urls(["https://a.com"], "tok1", None))

        # Case 2: Empty URLs
        self.assertTrue(purge_urls([], "tok1", "zone1"))
        mock_post.assert_not_called()

        # Case 3: Success path
        mock_response = MagicMock()
        mock_response.status_code = 200
        mock_response.raise_for_status = __import__("unittest.mock").mock.MagicMock()
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
        # First call has 30, second call has 10
        self.assertEqual(len(mock_post.call_args_list[0][1]["json"]["files"]), 30)
        self.assertEqual(len(mock_post.call_args_list[1][1]["json"]["files"]), 10)

        # Case 5: API failure
        mock_post.reset_mock()
        mock_post.side_effect = Exception("API fail")
        self.assertFalse(purge_urls(["https://a.com"], "tok1", "zone1"))

    @patch("odoo.addons.cloudflare.utils.cloudflare_api.requests.post")
    def test_05_purge_tags(self, mock_post):
        # [@ANCHOR: test_purge_tags_api]
        # # Verified by [@ANCHOR: test_purge_tags_api]

        # Case 1: Missing credentials
        self.assertFalse(purge_tags(["tag1"], None, "zone1"))
        self.assertFalse(purge_tags(["tag1"], "tok1", None))

        # Case 2: Empty tags
        self.assertTrue(purge_tags([], "tok1", "zone1"))
        mock_post.assert_not_called()

        # Case 3: Success path
        mock_response = MagicMock()
        mock_response.status_code = 200
        mock_response.raise_for_status = __import__("unittest.mock").mock.MagicMock()
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
        mock_post.side_effect = Exception("API fail")
        self.assertFalse(purge_tags(["tag1"], "tok1", "zone1"))
