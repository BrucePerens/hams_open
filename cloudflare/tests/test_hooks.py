# -*- coding: utf-8 -*-
# Copyright © HAMS project. AGPL-3.0-or-later.

from odoo.addons.zero_sudo.tests.common import HamsTransactionCase
from odoo.tests import tagged

from odoo.addons.cloudflare.hooks import post_init_hook


@tagged('post_install', '-at_install')
class TestCloudflareHooks(HamsTransactionCase):
    """post_init_hook() had never been exercised. Unlike hams_s3/
    distributed_redis_cache/backup_management's post_init_hooks, this one
    doesn't register a daemon key -- it calls
    initialize_cloudflare_state(), which real-network-calls Cloudflare's
    API (get_zone_ruleset) per website, but only for a website that
    actually has a token/zone_id configured
    (_get_cloudflare_credentials()) -- confirmed by reading
    config_manager.py directly, not assumed. No website in this test DB
    has real Cloudflare credentials, so this is a real regression guard
    against install-time crashes (e.g. a NoneType/KeyError on the
    no-credentials path), not a mock of the network call."""

    def test_post_init_hook_completes_without_error_when_no_website_has_cloudflare_credentials(self):
        # Tests [@ANCHOR: cloudflare:COMM_post_init_hook]

        # Tests [@ANCHOR: cloudflare:COMM_initialize_cloudflare_state]
        self.env.ref("cloudflare.user_cloudflare_waf")
        get_zone_ruleset = self.safe_patch("odoo.addons.cloudflare.models.config_manager.get_zone_ruleset")
        backups = self.env["cloudflare.config.backup"].search_count([])
        rules = self.env["cloudflare.waf.rule"].search_count([])
        post_init_hook(self.env)
        get_zone_ruleset.assert_not_called()  # no website has credentials, so Cloudflare is never asked
        self.assertEqual(self.env["cloudflare.config.backup"].search_count([]), backups)
        self.assertEqual(self.env["cloudflare.waf.rule"].search_count([]), rules)

    def test_post_init_hook_is_idempotent_on_reinstall(self):
        post_init_hook(self.env)
        backups = self.env["cloudflare.config.backup"].search_count([])
        rules = self.env["cloudflare.waf.rule"].search_count([])
        post_init_hook(self.env)
        self.assertEqual(self.env["cloudflare.config.backup"].search_count([]), backups, "a second run adds no backups")
        self.assertEqual(self.env["cloudflare.waf.rule"].search_count([]), rules, "a second run adds no rules")
