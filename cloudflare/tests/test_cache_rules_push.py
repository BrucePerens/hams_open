# Copyright © Bruce Perens K6BP.
# SPDX-License-Identifier: AGPL-3.0-or-later

# -*- coding: utf-8 -*-
from odoo.exceptions import AccessError, UserError, ValidationError
from odoo.tests.common import tagged
from odoo.addons.zero_sudo.tests.common import HamsTransactionCase

BYPASS_EXPR = '(http.cookie contains "session_id")'


@tagged("post_install", "-at_install")
class TestCacheRulesPush(HamsTransactionCase):
    """Cache rule rows reach Cloudflare's http_request_cache_settings ruleset via mocked API calls."""

    def setUp(self):
        super().setUp()
        self.svc_uid = self.env["zero_sudo.security.utils"]._get_service_uid(
            "cloudflare.user_cloudflare_waf"
        )
        self.website = self.env["website"].get_current_website()
        self.website.write(
            {"cloudflare_api_token": "fake_token", "cloudflare_zone_id": "fake_zone"}
        )
        from odoo.addons.distributed_redis_cache.redis_cache import (
            invalidate_model_cache,
        )

        invalidate_model_cache("website", [self.website.id])
        self.env["cloudflare.cache.rule"].search([]).unlink()
        self.Rule = self.env["cloudflare.cache.rule"]
        self.mgr = self.env["cloudflare.config.manager"].with_user(self.svc_uid)
        base = "odoo.addons.cloudflare.models.config_manager."
        self.mock_get = self.safe_patch(base + "get_zone_ruleset")
        self.mock_update = self.safe_patch(base + "update_zone_ruleset")
        self.mock_create = self.safe_patch(base + "create_zone_ruleset")
        self.mock_get.return_value = None
        self.mock_update.return_value = (True, "Updated")
        self.mock_create.return_value = (True, "Created")

    def _make_pair(self, ttl=0):
        # Created cache-first on purpose: order of creation must not matter, sequence does.
        cache = self.Rule.create(
            {
                "name": "Cache public HTML",
                "sequence": 20,
                "rule_action": "cache",
                "expression": "true",
                "edge_cache_ttl": ttl,
            }
        )
        bypass = self.Rule.create(
            {
                "name": "Bypass cache when session_id cookie present",
                "sequence": 10,
                "rule_action": "bypass",
                "expression": BYPASS_EXPR,
            }
        )
        return bypass, cache

    def test_01_bypass_first_and_cache_rule_excludes_bypass(self):
        # Tests [@ANCHOR: cf_action_push_cache_rules]
        self._make_pair()
        ok, _msg = self.mgr.action_push_cache_rules(website_id=self.website.id)
        self.assertTrue(ok)
        self.mock_create.assert_called_once()
        payload = self.mock_create.call_args[0][0]
        self.assertEqual(payload["phase"], "http_request_cache_settings")
        self.assertEqual(payload["kind"], "zone")
        first, second = payload["rules"]
        self.assertEqual(first["action"], "set_cache_settings")
        self.assertEqual(first["action_parameters"], {"cache": False})
        self.assertEqual(first["expression"], BYPASS_EXPR)
        self.assertEqual(
            second["action_parameters"],
            {"cache": True, "edge_ttl": {"mode": "respect_origin"}},
        )
        # A request the bypass rule matches can never match the cache rule, regardless of order.
        self.assertEqual(
            second["expression"], "(true) and not (%s)" % BYPASS_EXPR
        )

    def test_02_positive_ttl_overrides_origin(self):
        self._make_pair(ttl=3600)
        self.mgr.action_push_cache_rules(website_id=self.website.id)
        rule = self.mock_create.call_args[0][0]["rules"][1]
        self.assertEqual(
            rule["action_parameters"]["edge_ttl"],
            {"mode": "override_origin", "default": 3600},
        )

    def test_03_existing_ruleset_is_updated_not_recreated(self):
        self._make_pair()
        self.mock_get.return_value = {"id": "rs_1"}
        ok, _msg = self.mgr.action_push_cache_rules(website_id=self.website.id)
        self.assertTrue(ok)
        self.mock_create.assert_not_called()
        self.mock_update.assert_called_once()
        self.assertEqual(self.mock_update.call_args[0][0], "rs_1")
        self.assertEqual(
            self.mock_get.call_args[0][0], "http_request_cache_settings"
        )

    def test_04_cache_rule_without_session_bypass_is_refused(self):
        self.Rule.create(
            {"name": "Cache everything", "rule_action": "cache", "expression": "true"}
        )
        ok, msg = self.mgr.action_push_cache_rules(website_id=self.website.id)
        self.assertFalse(ok)
        self.assertIn("session_id", msg)
        self.mock_create.assert_not_called()
        self.mock_update.assert_not_called()

    def test_05_archived_bypass_does_not_count(self):
        bypass, _cache = self._make_pair()
        bypass.active = False
        ok, _msg = self.mgr.action_push_cache_rules(website_id=self.website.id)
        self.assertFalse(ok)
        self.mock_create.assert_not_called()

    def test_06_other_websites_rules_are_not_pushed(self):
        self._make_pair()
        other = self.env["website"].create({"name": "Other site"})
        self.Rule.create(
            {
                "name": "Other bypass",
                "rule_action": "bypass",
                "expression": '(http.host eq "other.example")',
                "website_id": other.id,
            }
        )
        self.mgr.action_push_cache_rules(website_id=self.website.id)
        names = [r["description"] for r in self.mock_create.call_args[0][0]["rules"]]
        self.assertNotIn("Other bypass", names)
        self.assertEqual(len(names), 2)

    def test_07_missing_credentials_makes_no_api_call(self):
        self._make_pair()
        self.website.write({"cloudflare_api_token": False})
        from odoo.addons.distributed_redis_cache.redis_cache import (
            invalidate_model_cache,
        )

        invalidate_model_cache("website", [self.website.id])
        ok, _msg = self.mgr.action_push_cache_rules(website_id=self.website.id)
        self.assertFalse(ok)
        self.mock_create.assert_not_called()
        self.mock_update.assert_not_called()

    def test_08_ordinary_user_cannot_push(self):
        user = self.env["res.users"].create(
            {
                "name": "Plain",
                "login": "plain_cache_rule_user",
                "group_ids": [(6, 0, [self.env.ref("base.group_user").id])],
            }
        )
        with self.assertRaises(AccessError):
            self.env["cloudflare.config.manager"].with_user(
                user
            ).action_push_cache_rules(website_id=self.website.id)

    def test_09_negative_ttl_and_empty_expression_rejected(self):
        with self.assertRaises(ValidationError):
            self.Rule.create({"name": "x", "edge_cache_ttl": -1})
        with self.assertRaises(Exception):
            with self.env.cr.savepoint():
                self.Rule.create({"name": "y", "expression": "  "})

    def test_10_button_failure_raises_and_success_notifies(self):
        self._make_pair()
        res = self.Rule.action_push_to_cloudflare()
        self.assertEqual(res["tag"], "display_notification")
        self.mock_create.return_value = (False, "API Error")
        with self.assertRaises(UserError):
            self.Rule.action_push_to_cloudflare()
