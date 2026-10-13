# Copyright © Bruce Perens K6BP.
# SPDX-License-Identifier: AGPL-3.0-or-later
# -*- coding: utf-8 -*-
from odoo.tests.common import tagged
from odoo.addons.zero_sudo.tests.common import HamsTransactionCase
from odoo.addons.distributed_redis_cache.redis_cache import invalidate_model_cache

PHASE = "http_request_cache_settings"


@tagged("post_install", "-at_install")
class TestCacheRulePush(HamsTransactionCase):
    """Odoo's cloudflare.cache.rule rows reach Cloudflare's cache-settings ruleset (mocked API)."""

    def setUp(self):
        super().setUp()
        self.svc_uid = self.env["zero_sudo.security.utils"]._get_service_uid(
            "cloudflare.user_cloudflare_waf"
        )
        self.website = self.env["website"].get_current_website()
        self.website.write(
            {"cloudflare_api_token": "fake_token", "cloudflare_zone_id": "fake_zone"}
        )
        invalidate_model_cache(self.env, "website")
        self.env["cloudflare.cache.rule"].search([]).unlink()
        self.mock_get = self.safe_patch(
            "odoo.addons.cloudflare.models.config_manager.get_zone_ruleset_checked"
        )
        self.mock_update = self.safe_patch(
            "odoo.addons.cloudflare.models.config_manager.update_zone_ruleset"
        )
        self.mock_create = self.safe_patch(
            "odoo.addons.cloudflare.models.config_manager.create_zone_ruleset"
        )
        self.mock_update.return_value = (True, "Updated")
        self.mock_update.side_effect = None
        self.mock_create.return_value = (True, "Created")

    def _make_rows(self):
        Rule = self.env["cloudflare.cache.rule"]
        # The cache rule is given the LOWER sequence on purpose: the bypass must still go first.
        Rule.create(
            {
                "name": "Cache public HTML, honour Cloudflare-CDN-Cache-Control",
                "action": "cache",
                "sequence": 1,
                "edge_cache_ttl": 0,
            }
        )
        Rule.create(
            {
                "name": "Bypass cache when session_id cookie present",
                "action": "bypass",
                "sequence": 50,
                "expression": '(http.cookie contains "session_id")',
            }
        )

    def _push(self):
        return (
            self.env["cloudflare.config.manager"]
            .with_user(self.svc_uid)
            .action_push_cache_rules(website_id=self.website.id)
        )

    def test_bypass_always_precedes_cache_and_respects_origin(self):
        self._make_rows()
        self.mock_get.return_value = ("ok", {"id": "rs_1", "name": "Existing", "rules": []})
        ok, _msg = self._push()
        self.assertTrue(ok)
        # Once to plan, once to read the zone back after the write.
        self.assertEqual(self.mock_get.call_count, 2)
        self.mock_get.assert_called_with(PHASE, "fake_token", "fake_zone")
        ruleset_id, payload = self.mock_update.call_args[0][:2]
        self.assertEqual(ruleset_id, "rs_1")
        self.assertEqual(payload["phase"], PHASE)
        rules = payload["rules"]
        self.assertEqual(len(rules), 2)
        self.assertEqual(rules[0]["action_parameters"], {"cache": False})
        self.assertEqual(rules[0]["expression"], '(http.cookie contains "session_id")')
        self.assertEqual(
            rules[1]["action_parameters"],
            {"cache": True, "edge_ttl": {"mode": "respect_origin"}, "browser_ttl": {"mode": "respect_origin"}},
        )
        # The cache rule carries the bypass condition's negation itself: every matching rule applies in
        # Cloudflare, so the bypass in front would not stop it caching a request that has the cookie.
        self.assertEqual(rules[1]["expression"], 'not ((http.cookie contains "session_id"))')
        self.assertEqual({r["action"] for r in rules}, {"set_cache_settings"})

    def test_positive_ttl_overrides_origin(self):
        self.env["cloudflare.cache.rule"].create(
            {"name": "Static", "action": "cache", "edge_cache_ttl": 600}
        )
        # Absent when planned; the read-back after the create finds the ruleset (no unmanaged rules).
        self.mock_get.side_effect = [("absent", None), ("ok", {"id": "rs_new", "name": "New", "rules": []})]
        ok, _msg = self._push()
        self.assertTrue(ok)
        self.mock_create.assert_called_once()
        self.mock_update.assert_not_called()
        params = self.mock_create.call_args[0][0]["rules"][0]["action_parameters"]
        self.assertEqual(
            params,
            {"cache": True, "edge_ttl": {"mode": "override_origin", "default": 600}, "browser_ttl": {"mode": "respect_origin"}},
        )

    def test_inactive_and_other_website_rules_not_pushed(self):
        Rule = self.env["cloudflare.cache.rule"]
        other = self.env["website"].create({"name": "Other site"})
        Rule.create({"name": "Off", "active": False})
        Rule.create({"name": "Elsewhere", "website_id": other.id})
        Rule.create({"name": "Here", "website_id": self.website.id})
        self.mock_get.return_value = ("ok", {"id": "rs_1", "name": "Existing", "rules": []})
        self._push()
        rules = self.mock_update.call_args[0][1]["rules"]
        self.assertEqual([r["description"] for r in rules], ["Here"])

    def test_missing_credentials_pushes_nothing(self):
        self.website.write({"cloudflare_api_token": False})
        invalidate_model_cache(self.env, "website")
        self._make_rows()
        ok, msg = self._push()
        self.assertFalse(ok)
        self.mock_update.assert_not_called()
        self.mock_create.assert_not_called()

    def test_form_button_pushes_via_service_user(self):
        self._make_rows()
        self.mock_get.return_value = ("ok", {"id": "rs_1", "name": "Existing", "rules": []})
        rec = self.env["cloudflare.cache.rule"].search([], limit=1)
        res = rec.action_push_to_cloudflare()
        self.assertEqual(res["params"]["type"], "success")
        self.mock_update.assert_called_once()

    def test_unmanaged_dashboard_rules_are_carried_through_untouched(self):
        self._make_rows()
        dash = {
            "id": "dash1", "version": "3", "last_updated": "x", "action": "set_cache_settings",
            "action_parameters": {"cache": True, "edge_ttl": {"mode": "override_origin", "default": 60}},
            "expression": '(http.host eq "x.example")', "description": "Dashboard rule", "enabled": True,
        }
        self.mock_get.return_value = ("ok", {"id": "rs_1", "name": "Existing", "rules": [dict(dash)]})
        ok, _msg = self._push()
        self.assertTrue(ok)
        rules = self.mock_update.call_args[0][1]["rules"]
        self.assertEqual([r["description"] for r in rules], [
            "Bypass cache when session_id cookie present", "Dashboard rule",
            "Cache public HTML, honour Cloudflare-CDN-Cache-Control"])
        kept = rules[1]
        self.assertEqual(kept["id"], "dash1")
        self.assertNotIn("version", kept)
        self.assertEqual(kept["expression"], dash["expression"])
        self.assertEqual(self.mock_update.call_args[0][1]["name"], "Existing")

    def test_unreadable_ruleset_is_refused_not_treated_as_empty(self):
        self._make_rows()
        self.mock_get.return_value = ("error", "API Error")
        ok, msg = self._push()
        self.assertFalse(ok)
        self.assertIn("REFUSED", msg)
        self.mock_update.assert_not_called()
        self.mock_create.assert_not_called()

    def test_plan_changes_nothing_and_stale_hash_is_refused(self):
        self._make_rows()
        self.mock_get.return_value = ("absent", None)
        plan = self.env["cloudflare.config.manager"].with_user(self.svc_uid).plan_cache_rules(self.website.id)
        self.assertTrue(plan["ok"])
        self.assertEqual(plan["writes"], 2)
        self.mock_create.assert_not_called()
        ok, msg = self.env["cloudflare.config.manager"].with_user(self.svc_uid).action_push_cache_rules(
            website_id=self.website.id, plan_hash="0" * 64)
        self.assertFalse(ok)
        self.mock_create.assert_not_called()

    def test_second_push_of_same_rows_has_nothing_to_do(self):
        self._make_rows()
        self.mock_get.return_value = ("absent", None)
        self._push()
        pushed = self.mock_create.call_args[0][0]["rules"]
        existing = [dict(r, id=f"r{i}") for i, r in enumerate(pushed)]
        self.mock_get.return_value = ("ok", {"id": "rs_1", "name": "n", "rules": existing})
        self.mock_update.reset_mock()
        ok, msg = self._push()
        self.assertTrue(ok)
        self.assertIn("Nothing to do", msg)
        self.mock_update.assert_not_called()
