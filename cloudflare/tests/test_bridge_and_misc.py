# Copyright © Bruce Perens K6BP.
# SPDX-License-Identifier: AGPL-3.0-or-later

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
import os
from unittest.mock import MagicMock
from cryptography.fernet import Fernet
from odoo.tests.common import tagged
from odoo.addons.zero_sudo.tests.real_transaction import RealTransactionCase
from odoo.addons.cloudflare.utils.cloudflare_api import update_cfd_tunnel_configuration


@tagged("post_install", "-at_install")
class TestBridgeAndMisc(RealTransactionCase):
    def setUp(self):
        super().setUp()
        self.PurgeQueue = self.env["cloudflare.purge.queue"]
        self.website = self.env["website"].get_current_website()
        self.PurgeQueue.search([], limit=10000).unlink()

    # Tests [@ANCHOR: cloudflare_purge_queue_cloudflare_purge_queue]
    # Tests [@ANCHOR: cloudflare_domain_cloudflare_routing_domain]
    # Tests [@ANCHOR: cloudflare_dns_record_cloudflare_dnsrecord]
    # Tests [@ANCHOR: cloudflare_bridge_product_template]
    # Tests [@ANCHOR: cloudflare_bridge_website_menu]
    # Tests [@ANCHOR: cloudflare_bridge_blog_post]
    # Tests [@ANCHOR: cloudflare_bridge_website_page]
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

    # Tests [@ANCHOR: cloudflare_zone_settings_wizard_cloudflare_zone_settings_wizard]
    # Tests [@ANCHOR: cloudflare_waf_rule_cloudflare_waf_rule]
    # Tests [@ANCHOR: cloudflare_tunnel_cloudflare_tunnel]
    # Tests [@ANCHOR: cloudflare_purge_wizard_cloudflare_purge_wizard]
    # Tests [@ANCHOR: cloudflare_ip_ban_cloudflare_ipban]
    # Tests [@ANCHOR: cloudflare_hostname_pending_delete_cloudflare_hostname_pending_delete]
    # Tests [@ANCHOR: cloudflare_config_backup_cloudflare_config_backup]
    # Tests [@ANCHOR: cloudflare_cloudflare_features_cloudflare_zero_trust_policy]
    # Tests [@ANCHOR: cloudflare_cloudflare_features_cloudflare_cache_rule]
    # Tests [@ANCHOR: cloudflare_cloudflare_features_cloudflare_rate_limit]
    # Tests [@ANCHOR: cloudflare_cloudflare_features_cloudflare_zone_settings]
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

    def test_blog_post_unlink_enqueues_a_purge(self):
        # Tests [@ANCHOR: cloudflare:COMM_blog_post_unlink]

        # BlogPost needs an unlink() override (write() is covered, so
        # archiving via active=False purges correctly, but a real hard delete
        # would leave the now-404 URL cached at Cloudflare's edge forever).
        blog = self.env["blog.blog"].create({"name": "Bridge Test Blog Unlink"})
        post = self.env["blog.post"].create(
            {"name": "Bridge Test Post Unlink", "blog_id": blog.id}
        )
        website_url = post.website_url
        self.PurgeQueue.search([], limit=10000).unlink()
        post.unlink()
        if website_url:
            self.assertTrue(
                self.PurgeQueue.search_count(
                    [("target_item", "like", "%" + website_url)]
                )
            )

    def test_product_template_unlink_enqueues_a_purge(self):
        # Tests [@ANCHOR: cloudflare:COMM_product_unlink]

        # Purge on hard delete too, same as BlogPost.unlink() above.
        product = self.env["product.template"].create(
            {"name": "Bridge Test Product Unlink"}
        )
        website_url = product.website_url
        self.PurgeQueue.search([], limit=10000).unlink()
        product.unlink()
        if website_url:
            self.assertTrue(
                self.PurgeQueue.search_count(
                    [("target_item", "like", "%" + website_url)]
                )
            )

    # Tests [@ANCHOR: cloudflare_website_website_cloudflare]
    # Tests [@ANCHOR: cloudflare_res_config_settings_res_config_settings]
    # Tests [@ANCHOR: cloudflare_config_manager_cloudflare_config_manager]
    def test_trigger_edge_purge_static_assets_enqueues_on_mtime_increase(self):
        # Tests [@ANCHOR: cloudflare:COMM_trigger_edge_purge_static_assets]
        #
        # Bug fix (2026-09-22): this write() used to rely on no crypto
        # secret being configured in this RealTransactionCase-based class
        # (unlike HamsTransactionCase, which provisions one in
        # setUpClass()) -- _crypt_field() silently discarded the token into
        # a `False` write with no error, so the missing-secret case and a
        # successful write were indistinguishable, and this test happened
        # to pass either way. Now that a missing crypto secret raises
        # instead of failing silently (per website.py's `_crypt_field`),
        # give this write a real key the same way HamsTransactionCase does,
        # so it actually round-trips instead of erroring.
        os.environ.setdefault("HAMS_CRYPTO_KEY", Fernet.generate_key().decode("utf-8"))
        self.website.write({"cloudflare_api_token": "tok", "cloudflare_zone_id": "zone"})
        self.safe_patch(
            "odoo.addons.cloudflare.models.website.WebsiteCloudflare._get_fernet",
            return_value=None,
        )
        # With the real encryption/credentials chain covered by the write()
        # above, mock _get_cloudflare_credentials directly for the rest of
        # this test so it only has to prove the mtime-comparison/purge-
        # enqueue logic, not the (already separately covered) encryption
        # chain.
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

    # Tests [@ANCHOR: cloudflare_edge_context_cloudflare_utils]
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

    # Tests [@ANCHOR: cloudflare_tunnel_route_cloudflare_tunnel_route]
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

    def test_get_cloudflare_credentials_treats_a_decrypt_error_sentinel_as_no_token(self):
        # On a genuine decrypt failure, _crypt_field() returns the literal string
        # "***ERROR***" (a deliberate, established cross-module UI convention, not
        # changed here), but every real caller of _get_cloudflare_credentials()
        # gates on plain truthiness (`if token and zone_id:`, `if not token:` --
        # confirmed directly in domain.py/tunnel.py/purge_wizard.py and others).
        # "***ERROR***" is truthy, so without filtering those callers would proceed to
        # call the real Cloudflare API with the literal string "***ERROR***" as the
        # bearer token instead of failing fast locally. Mocking the compute method
        # directly (not orchestrating a real key-rotation/InvalidToken scenario,
        # which is already covered by crypt_field's own tests) isolates exactly the
        # new filtering logic in _get_cloudflare_credentials itself.
        # Tests [@ANCHOR: cloudflare:COMM_get_cloudflare_credentials]
        self.website.write({"cloudflare_zone_id": "zone123"})
        self.safe_patch(
            "odoo.addons.cloudflare.models.website.WebsiteCloudflare._compute_cf_api_token",
            new=lambda self: setattr(self, "cloudflare_api_token", "***ERROR***"),
        )
        self.website.invalidate_recordset(["cloudflare_api_token"])

        # redis_bypass_cache: _get_cloudflare_credentials is @distributed_cache()'d,
        # and its module-level L1 fallback cache is a plain process-global dict --
        # shared across every test in this worker process, not scoped per-test.
        # Bypassing it here is what makes this test actually exercise today's
        # real code rather than risk a stale cached tuple from an earlier test's
        # own real (unmocked) call on this same "current website" record.
        token, zone = self.website.with_context(
            redis_bypass_cache=True
        )._get_cloudflare_credentials()

        self.assertFalse(
            token,
            "[!] DIAGNOSTIC FOR AI: a decrypt-error sentinel must never be returned as "
            "if it were a real, usable API token -- every caller's own `if token:` "
            "gate would treat the literal string \"***ERROR***\" as present and "
            "proceed to call the real Cloudflare API with it.",
        )
        self.assertEqual(zone, "zone123")
