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
            "odoo.addons.cloudflare.models.config_manager.get_zone_ruleset"
        )
        self.mock_update = self.safe_patch(
            "odoo.addons.cloudflare.models.config_manager.update_zone_ruleset"
        )
        self.mock_create = self.safe_patch(
            "odoo.addons.cloudflare.models.config_manager.create_zone_ruleset"
        )
        self.mock_update.return_value = (True, "Updated")
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
        self.mock_get.return_value = {"id": "rs_1"}
        ok, _msg = self._push()
        self.assertTrue(ok)
        self.mock_get.assert_called_once_with(PHASE, "fake_token", "fake_zone")
        ruleset_id, payload = self.mock_update.call_args[0][:2]
        self.assertEqual(ruleset_id, "rs_1")
        self.assertEqual(payload["phase"], PHASE)
        rules = payload["rules"]
        self.assertEqual(len(rules), 2)
        self.assertEqual(rules[0]["action_parameters"], {"cache": False})
        self.assertEqual(rules[0]["expression"], '(http.cookie contains "session_id")')
        self.assertEqual(
            rules[1]["action_parameters"],
            {"cache": True, "edge_ttl": {"mode": "respect_origin"}},
        )
        self.assertEqual(rules[1]["expression"], "true")
        self.assertEqual({r["action"] for r in rules}, {"set_cache_settings"})

    def test_positive_ttl_overrides_origin(self):
        self.env["cloudflare.cache.rule"].create(
            {"name": "Static", "action": "cache", "edge_cache_ttl": 600}
        )
        self.mock_get.return_value = None
        ok, _msg = self._push()
        self.assertTrue(ok)
        self.mock_create.assert_called_once()
        self.mock_update.assert_not_called()
        params = self.mock_create.call_args[0][0]["rules"][0]["action_parameters"]
        self.assertEqual(
            params,
            {"cache": True, "edge_ttl": {"mode": "override_origin", "default": 600}},
        )

    def test_inactive_and_other_website_rules_not_pushed(self):
        Rule = self.env["cloudflare.cache.rule"]
        other = self.env["website"].create({"name": "Other site"})
        Rule.create({"name": "Off", "active": False})
        Rule.create({"name": "Elsewhere", "website_id": other.id})
        Rule.create({"name": "Here", "website_id": self.website.id})
        self.mock_get.return_value = {"id": "rs_1"}
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
        self.mock_get.return_value = {"id": "rs_1"}
        rec = self.env["cloudflare.cache.rule"].search([], limit=1)
        res = rec.action_push_to_cloudflare()
        self.assertEqual(res["params"]["type"], "success")
        self.mock_update.assert_called_once()
