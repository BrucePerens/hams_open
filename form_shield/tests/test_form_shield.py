# SPDX-License-Identifier: AGPL-3.0-or-later
import time
from datetime import timedelta

import redis

from odoo import fields
from odoo.addons.distributed_redis_cache.redis_pool import get_redis_connection
from odoo.addons.zero_sudo.tests.common import HamsTransactionCase
from odoo.tests import tagged

from .. import tokens
from ..hooks import post_init_hook
from ..models import form_shield as shield_module
from .test_support import reset_shield_state, shield_response

GUEST = "203.0.113.9"


@tagged("post_install", "-at_install", "form_shield")
class TestFormShield(HamsTransactionCase):
    def setUp(self):
        super().setUp()
        reset_shield_state(self.env)
        self.public = self.env.ref("base.public_user")
        self.shield = self.env(user=self.public)["form.shield"]
        self.visitor_address = GUEST
        self.safe_patch_object(
            type(self.shield), "_client_address", lambda model: self.visitor_address
        )

    def counters(self, form_id="contact"):
        rows = self.env["form.shield.rejection"].search([("form_id", "=", form_id)])
        return {row.reason: row.count for row in rows}

    # [@ANCHOR: form_shield:test_check_accepts_a_solved_challenge_once]
    def test_check_accepts_a_solved_challenge_once(self):
        # Tests [@ANCHOR: form_shield:redis_connection]
        # Tests [@ANCHOR: form_shield:claim_nonce]
        # Tests [@ANCHOR: form_shield:check]
        response = shield_response(self.env, "contact", GUEST)
        verdict = self.shield.check("contact", response)
        self.assertTrue(verdict.ok, verdict)
        replay = self.shield.check("contact", response)
        self.assertEqual((replay.outcome, replay.reason), (shield_module.SILENT, "replay"))
        self.assertEqual(self.counters(), {"accepted": 1, "replay": 1})

    # [@ANCHOR: form_shield:test_check_refusal_table]
    def test_check_refusal_table(self):
        # Tests [@ANCHOR: form_shield:int_param]
        # Tests [@ANCHOR: form_shield:reject]
        # Tests [@ANCHOR: form_shield:check_response]
        # Tests [@ANCHOR: form_shield:over_limit]
        # Tests [@ANCHOR: form_shield:split_response]
        # Tests [@ANCHOR: form_shield:check]
        S, R, Q = shield_module.SILENT, shield_module.RETRY, shield_module.QUESTION
        good = shield_response(self.env, "contact", GUEST, age=30)
        token, _, counter = good.partition("~")
        ttl = shield_module.DEFAULT_TTL
        cases = [
            ("no answer at all", "", (Q, "no_token")),
            ("no separator", token, (Q, "no_token")),
            ("garbage", "x~y", (S, "bad_format")),
            ("wrong form", shield_response(self.env, "other_form", GUEST), (S, "bad_solution")),
            ("wrong counter", token + "~" + str(int(counter) + 1), (S, "bad_solution")),
            ("not a number", token + "~abc", (S, "bad_solution")),
            ("forged signature", token[:-1] + ("0" if token[-1] != "0" else "1") + "~" + counter, (S, "bad_signature")),
            ("expired", shield_response(self.env, "contact", GUEST, age=ttl + 5), (R, "expired")),
            ("another network", shield_response(self.env, "contact", "198.51.100.7"), (R, "ip_changed")),
            ("too fast", shield_response(self.env, "contact", GUEST, age=0), (R, "too_fast")),
        ]
        for label, response, expected in cases:
            verdict = self.shield.check("contact", response)
            self.assertFalse(verdict.ok, label)
            self.assertEqual((verdict.outcome, verdict.reason), expected, label)
        # A failure that is not a replay leaves the challenge unspent: the good one still works.
        self.assertTrue(self.shield.check("contact", good).ok)

    def test_same_24_network_shares_a_challenge(self):
        # Tests [@ANCHOR: form_shield:ip_tag]
        response = shield_response(self.env, "contact", "203.0.113.200")
        self.assertTrue(self.shield.check("contact", response).ok)

    def test_honeypot_marks_a_program(self):
        # Tests [@ANCHOR: form_shield:check]
        response = shield_response(self.env, "contact", GUEST)
        verdict = self.shield.check("contact", response, honeypots=["", "http://spam.example"])
        self.assertEqual((verdict.outcome, verdict.reason), (shield_module.SILENT, "honeypot"))
        # The honeypot is checked first, so the good challenge was not spent.
        self.assertTrue(self.shield.check("contact", response).ok)

    # [@ANCHOR: form_shield:test_check_form_reads_the_standard_fields]
    def test_check_form_reads_the_standard_fields(self):
        # Tests [@ANCHOR: form_shield:check_form]
        post = {tokens.RESPONSE_FIELD: shield_response(self.env, "contact", GUEST)}
        self.assertTrue(self.shield.check_form("contact", post).ok)
        post = {tokens.RESPONSE_FIELD: shield_response(self.env, "contact", GUEST), tokens.HONEYPOT_FIELD: "x"}
        self.assertEqual(self.shield.check_form("contact", post).reason, "honeypot")
        question = shield_response(self.env, "contact", GUEST, kind=tokens.KIND_QUESTION)
        challenge, _, answer = question.partition("~")
        post = {tokens.QUESTION_CHALLENGE_FIELD: challenge, tokens.QUESTION_ANSWER_FIELD: answer}
        self.assertTrue(self.shield.check_form("contact", post).ok)
        self.assertEqual(self.shield.check_form("contact", {}).reason, "no_token")

    def test_question_fallback_flow(self):
        # Tests [@ANCHOR: form_shield:question_valid]
        issued = self.shield.issue(tokens.KIND_QUESTION)
        self.assertEqual(issued["kind"], "q")
        self.assertEqual(issued["difficulty"], 0)
        self.assertIn("plus", issued["question"])
        parsed, _ = tokens.parse_challenge(self.shield._signing_key(), issued["challenge"])
        first, second = tokens.question_numbers(parsed.nonce)
        # Asked just now: too early, whatever the answer.
        early = f"{issued['challenge']}~{first + second}"
        self.assertEqual(self.shield.check("contact", early).reason, "too_fast")
        wrong = shield_response(self.env, "contact", GUEST, kind=tokens.KIND_QUESTION)
        challenge = wrong.partition("~")[0]
        verdict = self.shield.check("contact", challenge + "~0")
        self.assertEqual((verdict.outcome, verdict.reason), (shield_module.RETRY, "wrong_answer"))
        right = shield_response(self.env, "contact", GUEST, kind=tokens.KIND_QUESTION)
        self.assertTrue(self.shield.check("contact", right).ok)

    def test_members_get_the_light_check(self):
        # Tests [@ANCHOR: form_shield:check]
        member = self.env["res.users"].create(
            {"name": "Member", "login": "shield_member", "email": "m@example.com",
             "group_ids": [(6, 0, [self.env.ref("base.group_portal").id])]}
        )
        shield = self.env(user=member)["form.shield"]
        # No challenge needed ...
        self.assertTrue(shield.check("contact", "").ok)
        # ... but the honeypot and the rate limits still apply.
        self.assertEqual(shield.check("contact", "", honeypots=["x"]).reason, "honeypot")
        limits = (("subject", 2, 3600),)
        self.assertTrue(shield.check("limited", "", limits=limits).ok)
        self.assertTrue(shield.check("limited", "", limits=limits).ok)
        self.assertEqual(shield.check("limited", "", limits=limits).outcome, shield_module.LIMITED)

    def test_rate_limits_by_network_target_and_form(self):
        # Tests [@ANCHOR: form_shield:check]
        limits = (("subject", 2, 3600), ("target", 2, 3600), ("global", 100, 3600))

        def attempt(address, target):
            self.visitor_address = address
            response = shield_response(self.env, "tight", address)
            return self.shield.check("tight", response, target=target, limits=limits)

        self.assertTrue(attempt("203.0.113.1", "/a").ok)
        self.assertTrue(attempt("203.0.113.1", "/b").ok)
        third = attempt("203.0.113.1", "/c")
        self.assertEqual((third.outcome, third.reason), (shield_module.LIMITED, "rate_subject"))
        # A different network is counted separately ...
        self.assertTrue(attempt("198.51.100.1", "/a").ok)
        # ... until the same page has been reported too often, from anywhere.
        fourth = attempt("192.0.2.1", "/a")
        self.assertEqual((fourth.outcome, fourth.reason), (shield_module.LIMITED, "rate_target"))
        self.assertTrue(attempt("192.0.2.2", "/other").ok)
        # The whole-form limit.
        tiny = (("global", 1, 3600),)
        self.visitor_address = "192.0.2.2"
        response = shield_response(self.env, "tiny", "192.0.2.2")
        self.assertTrue(self.shield.check("tiny", response, limits=tiny).ok)
        response = shield_response(self.env, "tiny", "192.0.2.2")
        self.assertEqual(self.shield.check("tiny", response, limits=tiny).reason, "rate_global")

    def test_redis_down_is_reported_not_skipped(self):
        # Tests [@ANCHOR: form_shield:check]
        response = shield_response(self.env, "contact", GUEST)

        def broken(model):
            raise redis.exceptions.ConnectionError("down")

        with self.safe_patch_object_cm(type(self.shield), "_redis", broken):
            verdict = self.shield.check("contact", response)
        self.assertEqual(
            (verdict.ok, verdict.outcome, verdict.reason),
            (False, shield_module.UNAVAILABLE, "unavailable"),
        )
        # The challenge was not spent by the failed attempt.
        self.assertTrue(self.shield.check("contact", response).ok)

    # [@ANCHOR: form_shield:test_counters_keep_no_personal_data]
    def test_counters_keep_no_personal_data(self):
        # Tests [@ANCHOR: form_shield:redis_key]
        # Tests [@ANCHOR: form_shield:rejection_name]
        # Tests [@ANCHOR: form_shield:count_upsert]
        # Tests [@ANCHOR: form_shield:count]
        # Tests [@ANCHOR: form_shield:service_account_acl]
        self.shield.check("contact", shield_response(self.env, "contact", GUEST))
        self.shield.check("contact", "x~y")
        self.shield.check("contact", "x~y")
        rows = self.env["form.shield.rejection"].search([("form_id", "=", "contact")])
        self.assertEqual(sorted(rows.mapped("count")), [1, 2])
        stored = {
            name for name, field in rows._fields.items()
            if field.store and name not in models_magic()
        }
        self.assertEqual(stored, {"day", "form_id", "reason", "count"})
        # Redis holds a keyed hash of the network and nothing that spells the address.
        client = get_redis_connection(self.env)
        keys = list(client.scan_iter(f"form_shield:{self.env.cr.dbname}:*"))
        self.assertTrue(keys)
        for key in keys:
            for fragment in ("203.0.113", "203-0-113"):
                self.assertNotIn(fragment, key)
        # Only the shield's own service account and administrators can touch the table.
        with self.assertRaises(Exception):
            self.env["form.shield.rejection"].with_user(self.public).search([])

    # [@ANCHOR: form_shield:test_purge_drops_old_counters]
    def test_purge_drops_old_counters(self):
        # Tests [@ANCHOR: form_shield:purge]
        # Tests [@ANCHOR: form_shield:cron_purge]
        model = self.env["form.shield.rejection"]
        today = fields.Date.context_today(model)
        model.create([
            {"day": today - timedelta(days=200), "form_id": "old", "reason": "x", "count": 5},
            {"day": today - timedelta(days=3), "form_id": "new", "reason": "x", "count": 1},
        ])
        self.assertEqual(model._cron_purge(), 1)
        self.assertEqual(model.search([("form_id", "in", ["old", "new"])]).mapped("form_id"), ["new"])

    # [@ANCHOR: form_shield:test_post_init_hook_seeds_key_once]
    def test_post_init_hook_seeds_key_once(self):
        # Tests [@ANCHOR: form_shield:signing_key]
        # Tests [@ANCHOR: form_shield:post_init_hook]
        params = self.env["ir.config_parameter"]
        key = params.get_param("form_shield.signing_key")
        self.assertGreaterEqual(len(key), 32)
        post_init_hook(self.env)
        self.assertEqual(params.get_param("form_shield.signing_key"), key)

    def test_issue_is_signed_and_bound_to_the_visitor(self):
        # Tests [@ANCHOR: form_shield:issue]
        before = time.time()
        issued = self.shield.issue()
        parsed, reason = tokens.parse_challenge(self.shield._signing_key(), issued["challenge"])
        self.assertIsNone(reason)
        self.assertEqual(parsed.kind, "pow")
        self.assertEqual(parsed.difficulty, issued["difficulty"])
        self.assertEqual(issued["difficulty"], shield_module.DEFAULT_DIFFICULTY)
        self.assertGreaterEqual(parsed.issued, int(before))
        self.assertEqual(parsed.tag, tokens.ip_tag(self.shield._signing_key(), GUEST))
        self.assertEqual(issued["min_age"], shield_module.DEFAULT_MIN_AGE)

    def test_template_accessibility_attributes(self):
        # Tests [@ANCHOR: form_shield:fields_template]
        html = str(self.env["ir.qweb"]._render("form_shield.fields", {"fallback_url": "/fallback"}))
        self.assertIn('role="status"', html)
        self.assertIn('aria-live="polite"', html)
        self.assertIn('name="fs_homepage"', html)
        self.assertIn('tabindex="-1"', html)
        self.assertIn('aria-hidden="true"', html)
        self.assertNotIn("display:none", html.replace(" ", ""))
        self.assertNotIn("d-none", html)
        self.assertIn('href="/fallback"', html)
        self.assertIn("<noscript>", html)
        self.assertIn("hidden", html)


def models_magic():
    return {"id", "create_date", "write_date", "create_uid", "write_uid", "display_name", "__last_update"}
