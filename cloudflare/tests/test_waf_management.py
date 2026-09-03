# This software is distributed under the terms of the Affero General Public License (AGPL-3).

# -*- coding: utf-8 -*-
import re

from odoo.tests.common import tagged
from odoo.addons.web.controllers.database import Database
from odoo.addons.zero_sudo.tests.common import HamsTransactionCase
from odoo.exceptions import AccessError, UserError
from odoo.addons.distributed_redis_cache.redis_cache import invalidate_model_cache

from ..models.config_manager import DEFAULT_WAF_RULES


@tagged("post_install", "-at_install")
class TestWafManagement(HamsTransactionCase):

    def setUp(self):
        super().setUp()
        self.svc_uid = self.env["zero_sudo.security.utils"]._get_service_uid(
            "cloudflare.user_cloudflare_waf"
        )
        self.website = self.env["website"].get_current_website()
        self.website.write(
            {"cloudflare_api_token": "fake_token", "cloudflare_zone_id": "fake_zone"}
        )
        # Adversarial security review, 2026-09-03, found debugging a flaky
        # new test: _get_cloudflare_credentials() is @distributed_cache()-
        # wrapped and keyed only on (model, record id, method, args) -- it
        # is NOT invalidated just because this write() above changed the
        # underlying field, and in this test environment nothing is
        # running the real cache_manager.py daemon that would normally
        # relay a write's own PostgreSQL NOTIFY into a real Redis
        # invalidation. Without this, a stale cached credential tuple from
        # an earlier test/run (on the same "current website" singleton
        # record) can silently outlive this setUp()'s own fresh write,
        # confirmed directly while debugging (website.cloudflare_api_token
        # read back as False despite this exact write, only resolved once
        # this explicit invalidation was added). Real production writes go
        # through website.write()'s own override, which is expected to
        # trigger this same invalidation via the real NOTIFY pipeline --
        # this test environment doesn't have that daemon running, so it's
        # done explicitly here instead.
        invalidate_model_cache(self.env, "website")

    def test_01_cf_execute_ban(self):
        # [@ANCHOR: COMM_test_cf_execute_ban]

        # Tests [@ANCHOR: COMM_cf_execute_ban]
        mock_ban_ip = self.safe_patch("odoo.addons.cloudflare.models.ip_ban.ban_ip")
        mock_ban_ip.return_value = (True, "fake_rule_123")

        res = (
            self.env["cloudflare.ip.ban"]
            .with_user(self.svc_uid)
            ._execute_ban("10.0.0.1", notes="Test Spam", website_id=self.website.id)
        )
        self.assertTrue(
            res,
            "[!] DIAGNOSTIC FOR AI: _execute_ban returned False, indicating an API or credential failure.",
        )

        ban_record = self.env["cloudflare.ip.ban"].search(
            [("ip_address", "=", "10.0.0.1")], limit=1
        )
        self.assertTrue(
            ban_record,
            "[!] DIAGNOSTIC FOR AI: No cloudflare.ip.ban record was created after _execute_ban.",
        )
        self.assertEqual(ban_record.state, "active")
        self.assertEqual(ban_record.cf_rule_id, "fake_rule_123")
        self.assertEqual(ban_record.website_id.id, self.website.id)

    def test_02_cf_action_lift_ban(self):
        # [@ANCHOR: COMM_test_cf_action_lift_ban]

        # Tests [@ANCHOR: COMM_cf_action_lift_ban]
        ban_record = self.env["cloudflare.ip.ban"].create(
            {
                "ip_address": "192.168.1.50",
                "cf_rule_id": "rule_999",
                "state": "active",
                "website_id": self.website.id,
            }
        )

        mock_unban_ip = self.safe_patch("odoo.addons.cloudflare.models.ip_ban.unban_ip")

        mock_unban_ip.return_value = (False, "Edge Offline")
        with self.assertRaises(UserError):
            ban_record.action_lift_ban()
        self.assertEqual(ban_record.state, "active")

        mock_unban_ip.return_value = (True, "Success")
        ban_record.action_lift_ban()
        self.assertEqual(
            ban_record.state,
            "lifted",
            "[!] DIAGNOSTIC FOR AI: action_lift_ban failed to transition state to 'lifted'. Check API mock and logic.",
        )

    def test_03_cf_action_pull_waf_rules(self):
        # Tests [@ANCHOR: cf_action_pull_waf_rules]
        self.env["cloudflare.waf.rule"].create(
            {
                "name": "Old Rule",
                "expression": 'http.request.uri == "/"',
                "website_id": self.website.id,
            }
        )

        mock_get_ruleset = self.safe_patch(
            "odoo.addons.cloudflare.models.config_manager.get_zone_ruleset"
        )
        mock_get_ruleset.return_value = {
            "rules": [
                {
                    "id": "abc",
                    "description": "Cloudflare Rule 1",
                    "action": "block",
                    "expression": "ip.src eq 1.1.1.1",
                    "enabled": True,
                }
            ]
        }

        success, _msg = (
            self.env["cloudflare.config.manager"]
            .with_user(self.svc_uid)
            .action_pull_waf_rules(website_id=self.website.id)
        )
        self.assertTrue(
            success,
            "[!] DIAGNOSTIC FOR AI: action_pull_waf_rules failed. Check get_zone_ruleset mock.",
        )

        rules = self.env["cloudflare.waf.rule"].search(
            [("website_id", "=", self.website.id)], limit=10000
        )
        self.assertEqual(
            len(rules),
            1,
            "[!] DIAGNOSTIC FOR AI: Pulling rules should have resulted in exactly 1 rule.",
        )
        self.assertEqual(rules[0].name, "Cloudflare Rule 1")

    def test_04_cf_action_push_waf_rules(self):
        # Tests [@ANCHOR: cf_action_push_waf_rules]
        self.env["cloudflare.waf.rule"].search([], limit=10000).unlink()
        self.env["cloudflare.waf.rule"].create(
            {
                "name": "Local Rule",
                "action": "managed_challenge",
                "expression": "ip.src eq 2.2.2.2",
                "website_id": self.website.id,
            }
        )

        mock_get = self.safe_patch(
            "odoo.addons.cloudflare.models.config_manager.get_zone_ruleset"
        )
        mock_update = self.safe_patch(
            "odoo.addons.cloudflare.models.config_manager.update_zone_ruleset"
        )
        self.safe_patch(
            "odoo.addons.cloudflare.models.config_manager.create_zone_ruleset"
        )

        mock_get.return_value = {"id": "ruleset_777"}
        mock_update.return_value = (True, "Updated")

        success, _msg = (
            self.env["cloudflare.config.manager"]
            .with_user(self.svc_uid)
            .action_push_waf_rules(website_id=self.website.id)
        )
        self.assertTrue(
            success,
            "[!] DIAGNOSTIC FOR AI: action_push_waf_rules failed. Check update_zone_ruleset mock.",
        )
        mock_update.assert_called_once()

        payload = mock_update.call_args[0][1]
        self.assertEqual(payload["rules"][0]["action"], "managed_challenge")

    def test_05_execute_ban_missing_website(self):
        # [@ANCHOR: COMM_test_05_execute_ban_missing_website]
        """Verify _execute_ban gracefully handles missing website context."""
        # Force get_current_website_id to return None
        mock_get_website_id = self.safe_patch_object(
            type(self.env["cloudflare.utils"]), "get_current_website_id"
        )
        mock_get_website_id.return_value = None

        # Should return False and fail gracefully without crashing
        res = (
            self.env["cloudflare.ip.ban"]
            .with_user(self.svc_uid)
            ._execute_ban("10.0.0.2", notes="Test Spam")
        )
        self.assertFalse(res)

        ban_record = self.env["cloudflare.ip.ban"].search(
            [("ip_address", "=", "10.0.0.2")], limit=1
        )
        self.assertTrue(ban_record)
        self.assertEqual(ban_record.state, "failed")
        self.assertIn("Missing Cloudflare credentials", ban_record.notes)

    def test_07_ban_ip_rejects_an_unprivileged_caller(self):
        # Adversarial security review, 2026-09-03: ban_ip is a public
        # @api.model method (directly RPC-reachable by any authenticated
        # session) that used to escalate straight to the WAF service
        # account with no check on who the real caller was -- any portal
        # user could trigger a real Cloudflare Firewall Access Rules API
        # call against any website. Must now be refused for a plain user
        # with none of the authorizing groups.
        unprivileged = self.env["res.users"].create(
            {
                "name": "Unprivileged WAF Caller",
                "login": "waf_unprivileged",
                "group_ids": [(6, 0, [self.env.ref("base.group_portal").id])],
            }
        )
        with self.assertRaises(
            AccessError,
            msg="[!] DIAGNOSTIC FOR AI: an unprivileged caller must not be able to trigger a real Cloudflare IP ban.",
        ):
            self.env["cloudflare.waf"].with_user(unprivileged).ban_ip(
                "203.0.113.5", website_id=self.website.id
            )

    def test_08_ban_ip_allows_a_real_waf_group_member(self):
        # The fix must not over-block: a genuine cloudflare.group_cloudflare_waf
        # member (not just the internal service account) must still be able
        # to call this directly. Mocked at _execute_ban itself (rather than
        # asserting on ban_ip()'s own full return value) to isolate exactly
        # what this test needs to prove -- that _check_waf_caller_authorized()
        # lets a real WAF-group caller reach _execute_ban at all -- from the
        # unrelated Fernet-encryption/service-account-resolution chain
        # _get_cloudflare_credentials() depends on, which this test
        # environment doesn't have fully provisioned (confirmed directly
        # while debugging this test: real credential resolution needs a
        # live cloudflare_encryption_key daemon.key.registry entry this
        # sandbox doesn't have, unrelated to the authorization fix itself).
        mock_execute_ban = self.safe_patch_object(
            type(self.env["cloudflare.ip.ban"]), "_execute_ban"
        )
        mock_execute_ban.return_value = True

        waf_admin = self.env["res.users"].create(
            {
                "name": "Real WAF Admin",
                "login": "waf_admin",
                "group_ids": [
                    (6, 0, [
                        self.env.ref("base.group_user").id,
                        self.env.ref("cloudflare.group_cloudflare_waf").id,
                    ])
                ],
            }
        )
        result = self.env["cloudflare.waf"].with_user(waf_admin).ban_ip(
            "203.0.113.6", website_id=self.website.id
        )
        self.assertTrue(result)
        mock_execute_ban.assert_called_once()

    def test_09_action_pull_and_push_waf_rules_reject_an_unprivileged_caller(self):
        # Adversarial security review, 2026-09-03: both action_pull_waf_rules
        # and action_push_waf_rules are public @api.model methods that call
        # website._get_cloudflare_credentials() -- a @distributed_cache()-
        # wrapped method whose cache key never includes self.env.user, so a
        # cache hit (realistically the common case) returned the site's
        # real, decrypted Cloudflare token without the field-level ACL
        # inside that method ever running. The check must happen in these
        # entry points themselves, on every call, not rely on the cached
        # method's own (skippable-on-hit) internal check.
        unprivileged = self.env["res.users"].create(
            {
                "name": "Unprivileged Config Caller",
                "login": "waf_config_unprivileged",
                "group_ids": [(6, 0, [self.env.ref("base.group_portal").id])],
            }
        )
        manager = self.env["cloudflare.config.manager"].with_user(unprivileged)
        with self.assertRaises(AccessError):
            manager.action_pull_waf_rules(website_id=self.website.id)
        with self.assertRaises(AccessError):
            manager.action_push_waf_rules(website_id=self.website.id)

    def test_06_database_manager_waf_rule_covers_every_real_route(self):
        # Regression test for a real gap found via the 2026-08-26 usability-audit run: the
        # "Protect Database Manager" rule's expression once only matched /odoo/database/manager
        # and /odoo/database/selector, which are not routes this installed Odoo version actually
        # serves -- the real routes (introspected below directly from the installed
        # odoo.addons.web.controllers.database.Database controller, not hardcoded, so an Odoo
        # upgrade that moves these routes makes this test fail loudly rather than silently
        # stop protecting anything) are all under /web/database/, including /web/database/drop
        # and /web/database/restore. This does a simple substring simulation of the wirefilter
        # expression (eq "<path>" or contains "<substring>"), not a full wirefilter parser --
        # sufficient to catch the exact class of regression this guards against.
        rule = next(
            r for r in DEFAULT_WAF_RULES if r["name"] == "Protect Database Manager"
        )
        self.assertEqual(
            rule["action"],
            "block",
            "[!] DIAGNOSTIC FOR AI: the database-manager WAF rule must block, not challenge or log.",
        )
        expression = rule["expression"]

        eq_paths = re.findall(r'uri\.path eq "([^"]+)"', expression)
        contains_substrings = re.findall(r'uri\.path contains "([^"]+)"', expression)

        real_routes = []
        for method_name in (
            "selector",
            "manager",
            "create",
            "duplicate",
            "drop",
            "backup",
            "restore",
            "change_password",
            "list",
        ):
            real_routes.extend(
                getattr(Database, method_name).original_routing["routes"]
            )
        self.assertTrue(real_routes, "[!] DIAGNOSTIC FOR AI: introspection found no routes at all.")

        for path in real_routes:
            covered = path in eq_paths or any(sub in path for sub in contains_substrings)
            self.assertTrue(
                covered,
                f"[!] DIAGNOSTIC FOR AI: real Odoo database-manager route {path!r} is not "
                f"matched by the WAF rule expression {expression!r} -- this route would be "
                "reachable unauthenticated in production.",
            )
