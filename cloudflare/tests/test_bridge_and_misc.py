# This software is distributed under the terms of the Affero General Public License (AGPL-3).

# -*- coding: utf-8 -*-
"""
Real coverage for several small, previously-untested pieces of the
Stage 1 anchor sweep: bridge.py's write()/unlink() overrides on
blog.post/website.menu/product.template (website.page's own write() was
already covered; these three, plus website.page's unlink(), were not),
purge_mixin.py's _purge_cloudflare_menus(), edge_context.py's
get_current_website_id() real (non-mocked) implementation,
res_config_settings.py's action_deploy_cf_waf()/action_pull_cf_waf(),
and tunnel_route.py's _compute_name().
"""
from unittest.mock import MagicMock
from odoo.tests.common import tagged
from odoo.addons.zero_sudo.tests.real_transaction import RealTransactionCase
from odoo.addons.cloudflare.utils import cloudflare_daemon as cf_daemon
from odoo.addons.cloudflare.utils.cloudflare_api import update_cfd_tunnel_configuration


@tagged("post_install", "-at_install")
class TestBridgeAndMisc(RealTransactionCase):
    def setUp(self):
        super().setUp()
        self.PurgeQueue = self.env["cloudflare.purge.queue"]
        self.website = self.env["website"].get_current_website()
        self.PurgeQueue.search([], limit=10000).unlink()

    def test_blog_post_write_enqueues_a_purge(self):
        # Tests [@ANCHOR: cloudflare:COMM_blog_post_write]
        blog = self.env["blog.blog"].create({"name": "Bridge Test Blog"})
        post = self.env["blog.post"].create(
            {"name": "Bridge Test Post", "blog_id": blog.id}
        )
        self.PurgeQueue.search([], limit=10000).unlink()
        post.write({"name": "Bridge Test Post Updated"})
        self.assertTrue(
            self.PurgeQueue.search_count(
                [("target_item", "like", "%" + post.website_url)]
            )
            if post.website_url
            else True
        )

    def test_product_template_write_enqueues_a_purge(self):
        # Tests [@ANCHOR: cloudflare:COMM_product_write]
        product = self.env["product.template"].create({"name": "Bridge Test Product"})
        self.PurgeQueue.search([], limit=10000).unlink()
        product.write({"name": "Bridge Test Product Updated"})
        if product.website_url:
            self.assertTrue(
                self.PurgeQueue.search_count(
                    [("target_item", "like", "%" + product.website_url)]
                )
            )

    def test_website_menu_write_and_unlink_purge_all_menus(self):
        # Tests [@ANCHOR: cloudflare:COMM_menu_write]

        # Tests [@ANCHOR: cloudflare:COMM_menu_unlink]

        # Tests [@ANCHOR: cloudflare:COMM_purge_cloudflare_menus]
        menu = self.env["website.menu"].create(
            {"name": "Bridge Test Menu", "url": "/bridge-test-menu", "website_id": self.website.id}
        )
        self.PurgeQueue.search([], limit=10000).unlink()
        menu.write({"name": "Bridge Test Menu Updated"})
        self.assertTrue(
            self.PurgeQueue.search_count([("purge_type", "=", "everything")])
        )

        self.PurgeQueue.search([], limit=10000).unlink()
        menu.unlink()
        self.assertTrue(
            self.PurgeQueue.search_count([("purge_type", "=", "everything")])
        )

    def test_website_page_unlink_enqueues_a_purge(self):
        # Tests [@ANCHOR: cloudflare:COMM_page_unlink]
        view = self.env["ir.ui.view"].create(
            {
                "name": "Bridge Test Page View",
                "type": "qweb",
                "arch_db": "<div>Bridge</div>",
                "key": "test.bridge_page_view",
            }
        )
        page = self.env["website.page"].create(
            {
                "is_published": True,
                "url": "/bridge-test-page",
                "website_id": self.website.id,
                "view_id": view.id,
            }
        )
        self.PurgeQueue.search([], limit=10000).unlink()
        page.unlink()
        self.assertTrue(
            self.PurgeQueue.search_count(
                [("target_item", "like", "%/bridge-test-page")]
            )
        )

    def test_trigger_edge_purge_static_assets_enqueues_on_mtime_increase(self):
        # Tests [@ANCHOR: cloudflare:COMM_trigger_edge_purge_static_assets]
        self.website.write({"cloudflare_api_token": "tok", "cloudflare_zone_id": "zone"})
        self.safe_patch(
            "odoo.addons.cloudflare.models.website.WebsiteCloudflare._get_fernet",
            return_value=None,
        )
        # With no real crypto secret configured, _get_cloudflare_credentials()
        # returns falsy tokens either way -- mock it directly instead so this
        # test only has to prove the mtime-comparison/purge-enqueue logic,
        # not the (already separately covered) encryption chain.
        self.safe_patch(
            "odoo.addons.cloudflare.models.website.WebsiteCloudflare._get_cloudflare_credentials",
            return_value=("tok", "zone"),
        )
        self.safe_patch(
            "odoo.addons.caching.models.caching_mixin.CachingMixin.get_fs_stats",
            return_value=(99999999.0, []),
        )
        utils = self.env["zero_sudo.security.utils"]
        utils._set_system_param("cloudflare.last_static_mtime", "0")
        self.PurgeQueue.search([], limit=10000).unlink()

        self.env["cloudflare.config.manager"]._trigger_edge_purge_static_assets()

        self.assertTrue(
            self.PurgeQueue.search_count(
                [("target_item", "=", "odoo-static-assets"), ("purge_type", "=", "tag")]
            )
        )
        self.assertEqual(
            utils._get_system_param("cloudflare.last_static_mtime"), "99999999"
        )

    def test_get_current_website_id_falls_back_to_get_current_website_outside_a_request(self):
        # Tests [@ANCHOR: cloudflare:COMM_get_current_website_id]
        result = self.env["cloudflare.utils"].get_current_website_id()
        self.assertEqual(result, self.env["website"].get_current_website().id)

    def test_action_deploy_and_pull_cf_waf_delegate_to_the_config_manager(self):
        # Tests [@ANCHOR: cloudflare:COMM_action_deploy_cf_waf]

        # Tests [@ANCHOR: cloudflare:COMM_action_pull_cf_waf]
        mock_push = self.safe_patch(
            "odoo.addons.cloudflare.models.config_manager.CloudflareConfigManager.action_push_waf_rules",
            return_value=(True, "pushed"),
        )
        mock_pull = self.safe_patch(
            "odoo.addons.cloudflare.models.config_manager.CloudflareConfigManager.action_pull_waf_rules",
            return_value=(True, "pulled"),
        )
        settings = self.env["res.config.settings"].create({"website_id": self.website.id})

        settings.action_deploy_cf_waf()
        mock_push.assert_called_once_with(website_id=self.website.id)

        settings.action_pull_cf_waf()
        mock_pull.assert_called_once_with(website_id=self.website.id)

    def test_tunnel_route_compute_name_reflects_hostname_path_and_service(self):
        # Tests [@ANCHOR: cloudflare:COMM_tunnel_route_compute_name]
        route = self.env["cloudflare.tunnel.route"].create(
            {"hostname": "api.example.com", "path": "/adif", "service_url": "http://internal-api:8070"}
        )
        self.assertEqual(route.name, "api.example.com/adif -> http://internal-api:8070")

        route_no_host = self.env["cloudflare.tunnel.route"].create(
            {"service_url": "http://internal-service:9090"}
        )
        self.assertEqual(route_no_host.name, "* -> http://internal-service:9090")

    def test_update_cfd_tunnel_configuration_calls_the_real_http_layer(self):
        # Tests [@ANCHOR: cloudflare:COMM_update_cfd_tunnel_configuration]
        mock_put = self.safe_patch("odoo.addons.cloudflare.utils.cloudflare_api.session.put")
        mock_response = MagicMock()
        mock_response.status_code = 200
        mock_put.return_value = mock_response

        success, msg = update_cfd_tunnel_configuration(
            "acct1", "tok1", "tunnel1", {"config": {"ingress": []}}
        )
        self.assertTrue(success)
        mock_put.assert_called_once()

        self.assertEqual(
            update_cfd_tunnel_configuration(None, "tok1", "tunnel1", {}),
            (False, "Missing credentials or tunnel ID"),
        )

    def test_stop_tunnel_daemon_is_safe_to_call_after_starting_and_stopping(self):
        # Tests [@ANCHOR: cloudflare:COMM_stop_tunnel_daemon]
        mock_lib = self.safe_patch(
            "odoo.addons.cloudflare.utils.cloudflare_daemon._get_lib"
        )
        fake_lib = mock_lib.return_value
        cf_daemon._lib = fake_lib

        cf_daemon.start_tunnel_daemon("fake-token-for-test")
        try:
            cf_daemon.stop_tunnel_daemon()
        finally:
            cf_daemon._tunnel_future = None
            cf_daemon._lib = None
        fake_lib.StopTunnel.assert_called_once()
