# Copyright © Bruce Perens K6BP.
# SPDX-License-Identifier: AGPL-3.0-or-later
# -*- coding: utf-8 -*-
from odoo.tests.common import tagged, BaseCase
from odoo.addons.zero_sudo.tests.common import HamsTransactionCase
from odoo.addons.distributed_redis_cache.redis_cache import invalidate_model_cache
from odoo.addons.cloudflare.utils import rate_limit_plan

MOD = "odoo.addons.cloudflare.models.rate_limit_push."


def rule(name, expression="(true)", action="block", **limit):
    ratelimit = {"characteristics": ["cf.colo.id", "ip.src"], "period": 60, "requests_per_period": 100, "mitigation_timeout": 600}
    ratelimit.update(limit)
    return {"action": action, "expression": expression, "description": name, "enabled": True, "ratelimit": ratelimit}


OTHER = {"id": "x1", "version": "2", "last_updated": "t", "action": "block", "expression": "(false)", "description": "Hand made",
         "enabled": True, "ratelimit": {"period": 10, "requests_per_period": 5}}


@tagged("post_install", "-at_install")
class TestRateLimitPlanMerge(BaseCase):
    """The pure merge: managed rules in, unmanaged rules carried through untouched."""

    def test_empty_zone_creates_and_unmanaged_survive_first(self):
        # Tests [@ANCHOR: cloudflare:COMM_rate_limit_plan_merge]
        # Tests [@ANCHOR: cloudflare:COMM_rate_limit_plan_summary]
        plan = rate_limit_plan.merge([dict(OTHER)], [rule("Login")], {"Login"})
        self.assertEqual([r["description"] for r in plan["rules"]], ["Hand made", "Login"])
        self.assertNotIn("version", plan["rules"][0])
        self.assertEqual({e["name"]: e["action"] for e in plan["entries"]}, {"Login": "create", "Hand made": "keep"})
        self.assertFalse(plan["problems"])

    def test_update_by_id_unchanged_is_no_write_inactive_is_deleted(self):
        # Tests [@ANCHOR: cloudflare:COMM_rate_limit_plan_view]
        # Tests [@ANCHOR: cloudflare:COMM_rate_limit_plan_merge]
        remote = [dict(rule("A"), id="a1"), dict(rule("B"), id="b1"), dict(rule("Off"), id="o1")]
        plan = rate_limit_plan.merge(remote, [rule("A"), rule("B", requests_per_period=7)], {"A", "B", "Off"})
        self.assertEqual({e["name"]: e["action"] for e in plan["entries"]}, {"A": "unchanged", "B": "update", "Off": "delete"})
        self.assertEqual({r["description"]: r["id"] for r in plan["rules"]}, {"A": "a1", "B": "b1"})

    def test_duplicates_are_refused_and_hash_follows_the_zone(self):
        # Tests [@ANCHOR: cloudflare:COMM_rate_limit_plan_merge]
        self.assertTrue(rate_limit_plan.merge([], [rule("A"), rule("A")], {"A"})["problems"])
        self.assertTrue(rate_limit_plan.merge([dict(rule("A"), id="1"), dict(rule("A"), id="2")], [rule("A")], {"A"})["problems"])
        a = rate_limit_plan.merge([], [rule("A")], {"A"})["hash"]
        b = rate_limit_plan.merge([dict(OTHER)], [rule("A")], {"A"})["hash"]
        self.assertNotEqual(a, b)

    def test_render(self):
        # Tests [@ANCHOR: cloudflare:COMM_rate_limit_plan_render]
        plan = rate_limit_plan.merge([dict(OTHER)], [rule("Login")], {"Login"})
        lines = rate_limit_plan.render(plan, "1 rule(s) now").splitlines()
        self.assertEqual(lines[0], "Zone rate-limiting ruleset: 1 rule(s) now")
        self.assertTrue(lines[1].startswith("CREATE"))
        self.assertTrue(lines[-1].startswith("Summary: 1 change(s)"))


@tagged("post_install", "-at_install")
class TestRateLimitPush(HamsTransactionCase):
    """Odoo's cloudflare.rate.limit rows reach the zone's rate-limiting ruleset (mocked API, never the real one)."""

    def setUp(self):
        super().setUp()
        self.svc_uid = self.env["zero_sudo.security.utils"]._get_service_uid("cloudflare.user_cloudflare_waf")
        self.website = self.env["website"].get_current_website()
        self.website.write({"cloudflare_api_token": "fake_token", "cloudflare_zone_id": "fake_zone"})
        invalidate_model_cache(self.env, "website")
        self.env["cloudflare.rate.limit"].search([]).unlink()
        self.mock_get = self.safe_patch(MOD + "get_zone_ruleset_checked")
        self.mock_update = self.safe_patch(MOD + "update_zone_ruleset")
        self.mock_create = self.safe_patch(MOD + "create_zone_ruleset")
        self.mock_update.side_effect = lambda rs_id, payload, token, zone: self._written(payload)
        self.mock_create.side_effect = lambda payload, token, zone: self._written(payload)
        self.row = self.env["cloudflare.rate.limit"].create(
            {"name": "Login", "match_criteria": '(http.request.uri.path eq "/web/login")', "mitigation_action": "managed_challenge",
             "requests_per_period": 20, "period": 60, "mitigation_timeout": 600}
        )

    def _written(self, payload):
        """The mocked zone now holds what was written, so the read-back sees it."""
        rules = [dict(r, id=r.get("id") or f"n{i}") for i, r in enumerate(payload["rules"])]
        self.mock_get.return_value = ("ok", {"id": "rs1", "name": payload["name"], "rules": rules})
        return True, "Written"

    def _mgr(self):
        return self.env["cloudflare.config.manager"].with_user(self.svc_uid)

    def _push(self, **kw):
        return self._mgr().action_push_rate_limits(website_id=self.website.id, **kw)

    def test_row_becomes_a_ratelimit_rule(self):
        # Tests [@ANCHOR: cloudflare:COMM_rate_limit_build_rules]
        rules, problems = self._mgr()._build_rate_limit_rules(self.website)
        self.assertFalse(problems)
        self.assertEqual(rules[0]["action"], "managed_challenge")
        self.assertEqual(rules[0]["ratelimit"]["requests_per_period"], 20)
        self.assertEqual(rules[0]["ratelimit"]["characteristics"], ["cf.colo.id", "ip.src"])

    def test_push_creates_then_second_push_has_nothing_to_do(self):
        # Tests [@ANCHOR: cloudflare:COMM_rate_limit_push]
        self.mock_get.return_value = ("absent", None)
        ok, msg = self._push()
        self.assertTrue(ok, msg)
        payload = self.mock_create.call_args[0][0]
        self.assertEqual(payload["phase"], "http_ratelimit")
        self.assertEqual(self.mock_create.call_args[0][1:], ("fake_token", "fake_zone"))
        existing = [dict(r, id="r1") for r in payload["rules"]]
        self.mock_get.return_value = ("ok", {"id": "rs1", "name": "n", "rules": existing})
        ok, msg = self._push()
        self.assertTrue(ok)
        self.assertIn("Nothing to do", msg)
        self.mock_update.assert_not_called()

    def test_unmanaged_rules_are_carried_through_by_update(self):
        # Tests [@ANCHOR: cloudflare:COMM_rate_limit_push]
        self.mock_get.return_value = ("ok", {"id": "rs1", "name": "Existing", "rules": [dict(OTHER)]})
        self._push()
        rules = self.mock_update.call_args[0][1]["rules"]
        self.assertEqual([r["description"] for r in rules], ["Hand made", "Login"])
        self.assertEqual(rules[0]["id"], "x1")
        self.assertEqual(self.mock_update.call_args[0][0], "rs1")

    def test_unreadable_ruleset_is_refused_not_treated_as_empty(self):
        # Tests [@ANCHOR: cloudflare:COMM_rate_limit_plan]
        self.mock_get.return_value = ("error", "HTTP 403")
        ok, msg = self._push()
        self.assertFalse(ok)
        self.assertIn("REFUSED", msg)
        self.mock_create.assert_not_called()
        self.mock_update.assert_not_called()

    def test_plan_writes_nothing_stale_hash_refused_and_bad_rows_refused(self):
        # Tests [@ANCHOR: cloudflare:COMM_rate_limit_plan]
        self.mock_get.return_value = ("absent", None)
        plan = self._mgr().plan_rate_limits(self.website.id)
        self.assertEqual((plan["ok"], plan["writes"]), (True, 1))
        self.mock_create.assert_not_called()
        ok, msg = self._push(plan_hash="0" * 64)
        self.assertFalse(ok)
        self.mock_create.assert_not_called()
        self.row.write({"match_criteria": False})
        ok, msg = self._push()
        self.assertFalse(ok)
        self.assertIn("Login", msg)
        self.mock_create.assert_not_called()

    def test_missing_credentials_and_failed_write_are_reported(self):
        # Tests [@ANCHOR: cloudflare:COMM_rate_limit_push]
        self.mock_get.return_value = ("absent", None)
        self.mock_create.side_effect = None
        self.mock_create.return_value = (False, "API Error")
        ok, msg = self._push()
        self.assertFalse(ok)
        self.assertIn("FAILED", msg)
        self.assertNotIn("fake_token", msg)
        self.website.write({"cloudflare_api_token": False})
        invalidate_model_cache(self.env, "website")
        ok, msg = self._push()
        self.assertFalse(ok)
        self.assertIn("Missing API credentials", msg)

    def test_inactive_row_is_removed_from_the_zone(self):
        # Tests [@ANCHOR: cloudflare:COMM_rate_limit_push]
        rules, _p = self._mgr()._build_rate_limit_rules(self.website)
        self.mock_get.return_value = ("ok", {"id": "rs1", "name": "n", "rules": [dict(rules[0], id="r1")]})
        self.row.write({"active": False})
        ok, msg = self._push()
        self.assertTrue(ok, msg)
        self.assertEqual(self.mock_update.call_args[0][1]["rules"], [])

    def test_rate_limit_website_resolves_id_or_current(self):
        # Tests [@ANCHOR: cloudflare_rate_limit_push_cloudflare_config_manager_rate_limit]
        # Tests [@ANCHOR: cloudflare:COMM_rate_limit_website]
        mgr = self._mgr()
        self.assertEqual(mgr._rate_limit_website(self.website.id), self.website)
        self.assertEqual(mgr._rate_limit_website(None), self.env["website"].get_current_website())

    def test_rate_limit_toast_shape(self):
        # Tests [@ANCHOR: cloudflare:COMM_rate_limit_toast]
        ok = self.row._rate_limit_toast("T", "M", True, False)
        self.assertEqual(ok["tag"], "display_notification")
        self.assertEqual(ok["params"], {"title": "T", "message": "M", "type": "success", "sticky": False})
        self.assertEqual(self.row._rate_limit_toast("T", "M", False, True)["params"]["type"], "danger")

    def test_form_buttons_plan_then_push(self):
        # Tests [@ANCHOR: cloudflare_rate_limit_push_cloudflare_rate_limit_buttons]
        # Tests [@ANCHOR: cloudflare:COMM_rate_limit_row_plan_push]
        # Tests [@ANCHOR: cloudflare:COMM_rate_limit_row_push_to_cloudflare]
        self.mock_get.return_value = ("absent", None)
        res = self.row.action_plan_rate_limit_push()
        self.assertIn("CREATE", res["params"]["message"])
        self.mock_create.assert_not_called()
        self.mock_get.return_value = ("error", "API Error")
        self.assertEqual(self.row.action_plan_rate_limit_push()["params"]["type"], "danger")
        self.mock_get.return_value = ("absent", None)
        res = self.row.action_push_rate_limit_to_cloudflare()
        self.assertEqual(res["params"]["type"], "success")
        self.mock_create.assert_called_once()
