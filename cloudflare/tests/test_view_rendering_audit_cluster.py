# -*- coding: utf-8 -*-
# Copyright © HAMS project. AGPL-3.0-or-later.
from odoo.tests.common import tagged
from odoo.addons.zero_sudo.tests.common import HamsTransactionCase


@tagged("post_install", "-at_install")
class TestViewRenderingAuditCluster(HamsTransactionCase):
    """
    Real coverage for COMM_test_cf_backend_views_rendering -- a single
    shared audit-ignore-view claim covering 19 record locations across
    cloudflare_features_views.xml, config_backup_views.xml, and
    tunnel_wizard_views.xml, none of which had any matching test before
    this (caught once verify_anchors.py's ADR-0054 gate was actually
    re-enabled, see hams_shared commit 1309d42). Each real ir.ui.view
    form/tree is rendered via get_view(), matching this codebase's own
    established test_all_xpaths_render convention; each ir.actions.*
    record (act_window and the one client action) is confirmed to
    resolve with the fields a menu item actually depends on -- these
    have no "arch" of their own to render, so a rendering test isn't the
    right shape of check for them.
    """

    def test_dns_record_views_render(self):
        # Tests [@ANCHOR: COMM_test_cf_backend_views_rendering]
        form = self.env["cloudflare.dns.record"].get_view(view_type="form")
        self.assertIn("content", form["arch"])
        tree = self.env["cloudflare.dns.record"].get_view(view_type="list")
        self.assertIn("proxied", tree["arch"])

    def test_zone_settings_views_render(self):
        form = self.env["cloudflare.zone.settings"].get_view(view_type="form")
        self.assertIn("bot_fight_mode", form["arch"])
        tree = self.env["cloudflare.zone.settings"].get_view(view_type="list")
        self.assertIn("ssl_mode", tree["arch"])

    def test_rate_limit_views_render(self):
        form = self.env["cloudflare.rate.limit"].get_view(view_type="form")
        self.assertIn("match_criteria", form["arch"])
        tree = self.env["cloudflare.rate.limit"].get_view(view_type="list")
        self.assertIn("mitigation_action", tree["arch"])

    def test_cache_rule_views_render(self):
        form = self.env["cloudflare.cache.rule"].get_view(view_type="form")
        self.assertIn("bypass_rules", form["arch"])
        tree = self.env["cloudflare.cache.rule"].get_view(view_type="list")
        self.assertIn("edge_cache_ttl", tree["arch"])

    def test_zero_trust_policy_views_render(self):
        form = self.env["cloudflare.zero.trust.policy"].get_view(view_type="form")
        self.assertIn("idps", form["arch"])
        tree = self.env["cloudflare.zero.trust.policy"].get_view(view_type="list")
        self.assertIn("policy_action", tree["arch"])

    def test_config_backup_views_render(self):
        form = self.env["cloudflare.config.backup"].get_view(view_type="form")
        self.assertIn("raw_json", form["arch"])
        tree = self.env["cloudflare.config.backup"].get_view(view_type="list")
        self.assertIn("phase", tree["arch"])

    def test_tunnel_wizard_view_renders(self):
        form = self.env["cloudflare.tunnel.wizard"].get_view(view_type="form")
        self.assertIn("command", form["arch"])

    def test_act_window_actions_resolve_to_the_right_model_and_views(self):
        expected = {
            "action_cf_dns_record": "cloudflare.dns.record",
            "action_cf_zone_settings": "cloudflare.zone.settings",
            "action_cf_rate_limit": "cloudflare.rate.limit",
            "action_cf_cache_rule": "cloudflare.cache.rule",
            "action_cf_zero_trust_policy": "cloudflare.zero.trust.policy",
            "action_cf_config_backup": "cloudflare.config.backup",
        }
        for xml_id, model in expected.items():
            action = self.env.ref(f"cloudflare.{xml_id}")
            self.assertEqual(action.res_model, model)

    def test_analytics_dashboard_client_action_resolves(self):
        # The client action itself has no "arch" -- its real content is
        # the OWL component registered under its own tag
        # (cloudflare_analytics_dashboard, static/src/components/
        # analytics/), a browser-level concern this Python test doesn't
        # cover. What's real to check here is that the action record a
        # menu item points at actually exists with the right tag.
        action = self.env.ref("cloudflare.action_cf_analytics_dashboard")
        self.assertEqual(action.tag, "cloudflare_analytics_dashboard")
