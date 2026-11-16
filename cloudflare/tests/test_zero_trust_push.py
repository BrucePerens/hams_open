# Copyright © Bruce Perens K6BP.
# SPDX-License-Identifier: AGPL-3.0-or-later
# -*- coding: utf-8 -*-
from odoo.tests.common import tagged, BaseCase
from odoo.addons.zero_sudo.tests.common import HamsTransactionCase
from odoo.addons.distributed_redis_cache.redis_cache import invalidate_model_cache
from odoo.addons.cloudflare.utils import access_api, zero_trust_plan

MOD = "odoo.addons.cloudflare.models.zero_trust_push."


class Resp:
    def __init__(self, status, body):
        self.status_code, self._body = status, body

    def json(self):
        return self._body


@tagged("post_install", "-at_install")
class TestZeroTrustPlan(BaseCase):
    """The pure comparison."""

    def test_row_to_policy(self):
        # Tests [@ANCHOR: cloudflare:COMM_zero_trust_plan_desired]
        d, problem = zero_trust_plan.desired_from_row("Staff", "allow", "idp1, idp2")
        self.assertIsNone(problem)
        self.assertEqual(d["decision"], "allow")
        self.assertEqual(d["include"], [{"login_method": {"id": "idp1"}}, {"login_method": {"id": "idp2"}}])
        d, _p = zero_trust_plan.desired_from_row("Block", "block", "")
        self.assertEqual((d["decision"], d["include"]), ("deny", [{"everyone": {}}]))

    def test_allow_without_provider_or_action_is_refused(self):
        # Tests [@ANCHOR: cloudflare:COMM_zero_trust_plan_desired]
        self.assertIn("everyone", zero_trust_plan.desired_from_row("X", "allow", "")[1])
        self.assertIsNotNone(zero_trust_plan.desired_from_row("X", False, "i")[1])

    def test_create_update_unchanged_and_nothing_deleted(self):
        # Tests [@ANCHOR: cloudflare:COMM_zero_trust_plan_make]
        # Tests [@ANCHOR: cloudflare:COMM_zero_trust_plan_view]
        a = zero_trust_plan.desired_from_row("A", "allow", "i1")[0]
        b = zero_trust_plan.desired_from_row("B", "block", "")[0]
        c = zero_trust_plan.desired_from_row("C", "bypass", "")[0]
        remote = [dict(a, id="a1", version=3), dict(b, id="b1", decision="allow"), {"id": "z1", "name": "Hand made", "decision": "allow"}]
        plan = zero_trust_plan.make(remote, [a, b, c])
        self.assertEqual({e["name"]: e["action"] for e in plan["entries"]}, {"A": "unchanged", "B": "update", "C": "create"})
        self.assertEqual([(w[0], w[1]) for w in plan["writes"]], [("update", "b1"), ("create", None)])
        self.assertFalse(plan["problems"])

    def test_duplicates_are_refused_and_hash_follows_the_account(self):
        # Tests [@ANCHOR: cloudflare:COMM_zero_trust_plan_make]
        a = zero_trust_plan.desired_from_row("A", "block", "")[0]
        self.assertTrue(zero_trust_plan.make([], [a, a])["problems"])
        self.assertTrue(zero_trust_plan.make([dict(a, id="1"), dict(a, id="2")], [a])["problems"])
        self.assertNotEqual(zero_trust_plan.make([], [a])["hash"], zero_trust_plan.make([{"id": "q", "name": "Q"}], [a])["hash"])

    def test_render(self):
        # Tests [@ANCHOR: cloudflare:COMM_zero_trust_plan_render]
        a = zero_trust_plan.desired_from_row("A", "block", "")[0]
        lines = zero_trust_plan.render(zero_trust_plan.make([], [a])).splitlines()
        self.assertTrue(lines[0].startswith("CREATE"))
        self.assertTrue(lines[-1].startswith("Summary: 1 policy write(s)"))


@tagged("post_install", "-at_install")
class TestAccessApi(HamsTransactionCase):
    """The account-level calls, with the HTTP layer mocked."""

    def setUp(self):
        super().setUp()
        self.req = self.safe_patch("odoo.addons.cloudflare.utils.access_api._make_request")

    def test_list_reads_every_page_and_refuses_unreadable_answers(self):
        # Tests [@ANCHOR: cloudflare:COMM_access_api_list_policies_checked]
        self.req.side_effect = [
            Resp(200, {"result": [{"id": "1"}], "result_info": {"total_pages": 2}}),
            Resp(200, {"result": [{"id": "2"}], "result_info": {"total_pages": 2}}),
        ]
        self.assertEqual(access_api.list_access_policies_checked("acc", "t"), ("ok", [{"id": "1"}, {"id": "2"}]))
        self.req.side_effect = None
        self.req.return_value = Resp(403, {})
        self.assertEqual(access_api.list_access_policies_checked("acc", "t"), ("error", "HTTP 403"))
        self.req.return_value = None
        self.assertEqual(access_api.list_access_policies_checked("acc", "t")[0], "error")
        self.req.return_value = Resp(200, {"result": "nope"})
        self.assertEqual(access_api.list_access_policies_checked("acc", "t")[0], "error")
        self.assertEqual(access_api.list_access_policies_checked("", "t")[0], "error")

    def test_write_creates_or_updates(self):
        # Tests [@ANCHOR: cloudflare:COMM_access_api_write_policy]
        self.req.return_value = Resp(200, {})
        self.assertTrue(access_api.write_access_policy("acc", None, {"name": "n"}, "t")[0])
        self.assertEqual(self.req.call_args[0][0], "POST")
        access_api.write_access_policy("acc", "p1", {"name": "n"}, "t")
        self.assertEqual(self.req.call_args[0][0], "PUT")
        self.assertTrue(self.req.call_args[0][1].endswith("/access/policies/p1"))
        self.req.return_value = Resp(400, {})
        self.assertEqual(access_api.write_access_policy("acc", None, {}, "t"), (False, "API Error (HTTP 400)"))
        self.req.return_value = None
        self.assertEqual(access_api.write_access_policy("acc", None, {}, "t"), (False, "API Error"))
        self.assertFalse(access_api.write_access_policy("", None, {}, "t")[0])


@tagged("post_install", "-at_install")
class TestZeroTrustPush(HamsTransactionCase):
    """Odoo's cloudflare.zero.trust.policy rows reach the account (mocked API, never the real one)."""

    def setUp(self):
        super().setUp()
        self.svc_uid = self.env["zero_sudo.security.utils"]._get_service_uid("cloudflare.user_cloudflare_waf")
        self.website = self.env["website"].get_current_website()
        self.website.write({"cloudflare_api_token": "fake_token", "cloudflare_zone_id": "fake_zone", "cloudflare_account_id": "fake_acc"})
        invalidate_model_cache(self.env, "website")
        self.env["cloudflare.zero.trust.policy"].search([]).unlink()
        self.zone = []
        self.mock_list = self.safe_patch(MOD + "list_access_policies_checked")
        self.mock_list.side_effect = lambda acc, token: ("ok", [dict(p) for p in self.zone])
        self.mock_write = self.safe_patch(MOD + "write_access_policy")
        self.mock_write.side_effect = self._write
        Policy = self.env["cloudflare.zero.trust.policy"]
        self.staff = Policy.create({"name": "Staff", "policy_action": "allow", "idps": "idp1"})
        self.blocked = Policy.create({"name": "Blocked", "policy_action": "block"})

    def _write(self, account, policy_id, payload, token):
        self.zone[:] = [p for p in self.zone if p["id"] != policy_id]
        self.zone.append(dict(payload, id=policy_id or f"n{len(self.zone)}"))
        return True, "written"

    def _mgr(self):
        return self.env["cloudflare.config.manager"].with_user(self.svc_uid)

    def _push(self, **kw):
        return self._mgr().action_push_zero_trust_policies(website_id=self.website.id, **kw)

    def test_push_creates_each_policy_then_second_push_has_nothing_to_do(self):
        # Tests [@ANCHOR: cloudflare:COMM_zero_trust_push]
        ok, msg = self._push()
        self.assertTrue(ok, msg)
        self.assertEqual(self.mock_write.call_count, 2)
        self.assertEqual(self.mock_write.call_args[0][0], "fake_acc")
        self.assertIn("OK create Staff", msg)
        self.assertNotIn("fake_token", msg)
        self.mock_write.reset_mock()
        ok, msg = self._push()
        self.assertTrue(ok)
        self.assertIn("Nothing to do", msg)
        self.mock_write.assert_not_called()

    def test_changed_row_updates_only_that_policy_by_id(self):
        # Tests [@ANCHOR: cloudflare:COMM_zero_trust_push]
        self._push()
        self.mock_write.reset_mock()
        self.blocked.write({"policy_action": "bypass"})
        ok, msg = self._push()
        self.assertTrue(ok, msg)
        self.mock_write.assert_called_once()
        self.assertIsNotNone(self.mock_write.call_args[0][1])
        self.assertEqual(self.mock_write.call_args[0][2]["decision"], "bypass")

    def test_failed_write_is_reported_per_policy_and_others_still_tried(self):
        # Tests [@ANCHOR: cloudflare:COMM_zero_trust_push]
        def write(account, policy_id, payload, token):
            if payload["name"] == "Staff":
                return False, "API Error (HTTP 400)"
            return self._write(account, policy_id, payload, token)
        self.mock_write.side_effect = write
        ok, msg = self._push()
        self.assertFalse(ok)
        self.assertIn("FAILED create Staff: API Error (HTTP 400)", msg)
        self.assertIn("OK create Blocked", msg)

    def test_unreadable_list_is_refused_not_treated_as_empty(self):
        # Tests [@ANCHOR: cloudflare:COMM_zero_trust_plan]
        self.mock_list.side_effect = lambda acc, token: ("error", "HTTP 403")
        ok, msg = self._push()
        self.assertFalse(ok)
        self.assertIn("REFUSED", msg)
        self.mock_write.assert_not_called()

    def test_plan_writes_nothing_stale_hash_and_bad_rows_and_credentials_refused(self):
        # Tests [@ANCHOR: cloudflare:COMM_zero_trust_plan]
        plan = self._mgr().plan_zero_trust_policies(self.website.id)
        self.assertEqual((plan["ok"], plan["writes"]), (True, 2))
        self.mock_write.assert_not_called()
        ok, msg = self._push(plan_hash="0" * 64)
        self.assertFalse(ok)
        self.mock_write.assert_not_called()
        self.staff.write({"idps": False})
        ok, msg = self._push()
        self.assertFalse(ok)
        self.assertIn("Staff", msg)
        self.mock_write.assert_not_called()
        self.staff.write({"idps": "idp1"})
        self.website.write({"cloudflare_account_id": False})
        invalidate_model_cache(self.env, "website")
        ok, msg = self._push()
        self.assertFalse(ok)
        self.assertIn("Missing API token or account id", msg)

    def test_form_buttons_plan_then_push(self):
        # Tests [@ANCHOR: cloudflare:COMM_zero_trust_row_plan_push]
        # Tests [@ANCHOR: cloudflare:COMM_zero_trust_row_push_to_cloudflare]
        res = self.staff.action_plan_zero_trust_push()
        self.assertIn("CREATE", res["params"]["message"])
        self.mock_write.assert_not_called()
        self.mock_list.side_effect = lambda acc, token: ("error", "API Error")
        self.assertEqual(self.staff.action_plan_zero_trust_push()["params"]["type"], "danger")
        self.mock_list.side_effect = lambda acc, token: ("ok", [dict(p) for p in self.zone])
        res = self.staff.action_push_zero_trust_to_cloudflare()
        self.assertEqual(res["params"]["type"], "success")
        self.assertEqual(self.mock_write.call_count, 2)
