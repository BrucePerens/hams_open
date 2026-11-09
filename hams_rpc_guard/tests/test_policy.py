# -*- coding: utf-8 -*-
# Copyright © Bruce Perens K6BP.
# SPDX-License-Identifier: AGPL-3.0-or-later
from odoo.exceptions import AccessError
from odoo.tests import tagged
from odoo.addons.zero_sudo.tests.common import HamsTransactionCase

from odoo.addons.hams_rpc_guard import exposure, policy
from odoo.addons.hams_rpc_guard.policy import ALLOW, DENY, Call


def _exposure(internal=(), portal=(), stock=(), clients=None):
    result = exposure.Exposure()
    result.internal.update(internal)
    result.portal.update(portal)
    result.stock.update(stock)
    for login, pairs in (clients or {}).items():
        result.clients[login] = set(pairs)
    return result


@tagged("post_install", "-at_install")
class TestDecision(HamsTransactionCase):
    """The decision table. Tests the rules in order, with no database involved."""
    # Tests [@ANCHOR: hams_rpc_guard:decide]
    # Tests [@ANCHOR: hams_rpc_guard:declared]
    # Tests [@ANCHOR: hams_rpc_guard:call_init]

    def _call(self, user_class, **kw):
        values = dict(model="x.model", method="do_it", defining_models=("x.model",), managed=True)
        values.update(kw)
        return Call(user_class=user_class, **values)

    def test_internal_user_a_managed_method_must_be_listed(self):
        self.assertEqual(policy.decide(self._call("internal"), _exposure())[0], DENY)
        self.assertEqual(policy.decide(self._call("internal"), _exposure(internal=[("x.model", "do_it")]))[0], ALLOW)

    def test_a_pair_listed_under_the_mixin_name_counts(self):
        call = self._call("internal", model="x.concrete", defining_models=("x.mixin", "x.concrete"))
        self.assertEqual(policy.decide(call, _exposure(internal=[("x.mixin", "do_it")]))[0], ALLOW)

    def test_the_public_rpc_mark_lists_a_method(self):
        marked = self._call("internal", marks=({"portal": False, "clients": ()},))
        self.assertEqual(policy.decide(marked, _exposure())[0], ALLOW)

    def test_stock_methods_and_standard_orm_overrides_keep_core_behaviour(self):
        self.assertEqual(policy.decide(self._call("internal", managed=False), _exposure()), (ALLOW, "core"))
        self.assertEqual(policy.decide(self._call("internal", orm_standard=True), _exposure()), (ALLOW, "core"))

    def test_strict_stock_asks_for_a_listing_of_stock_methods_too(self):
        call = self._call("internal", managed=False)
        self.assertEqual(policy.decide(call, _exposure(), strict_stock=True), (DENY, "strict-stock"))
        self.assertEqual(policy.decide(call, _exposure(stock=[("x.model", "do_it")]), strict_stock=True)[0], ALLOW)

    def test_public_and_portal_users_are_strict_even_for_stock_methods(self):
        for user_class in ("public", "portal"):
            stock = self._call(user_class, managed=False, orm_standard=True)
            self.assertEqual(policy.decide(stock, _exposure(internal=[("x.model", "do_it")]))[0], DENY, user_class)
            self.assertEqual(policy.decide(stock, _exposure(portal=[("x.model", "do_it")]))[0], ALLOW, user_class)

    def test_the_portal_flag_of_the_mark_admits_a_portal_user_but_a_plain_mark_does_not(self):
        plain = self._call("portal", marks=({"portal": False, "clients": ()},))
        flagged = self._call("portal", marks=({"portal": True, "clients": ()},))
        self.assertEqual(policy.decide(plain, _exposure())[0], DENY)
        self.assertEqual(policy.decide(flagged, _exposure())[0], ALLOW)

    def test_a_daemon_may_call_only_what_its_own_entry_lists(self):
        listed = _exposure(internal=[("x.model", "do_it")], clients={"daemon_a": [("x.model", "do_it")], "daemon_b": [("y.model", "other")]})
        self.assertEqual(policy.decide(self._call("service", login="daemon_a"), listed)[0], ALLOW)
        # Listed for internal users, but not for this daemon: another family's method.
        self.assertEqual(policy.decide(self._call("service", login="daemon_b"), listed), (DENY, "client-not-listed"))

    def test_a_service_account_without_an_entry_is_judged_as_an_internal_user_and_flagged(self):
        verdict = policy.decide(self._call("service", login="nobody"), _exposure())
        self.assertEqual(verdict, (DENY, "unmapped-client-managed-not-listed"))
        self.assertEqual(policy.decide(self._call("service", login="nobody", managed=False), _exposure()),
                         (ALLOW, "unmapped-client-core"))

    def test_the_certificate_subject_can_only_narrow_a_daemon(self):
        both = _exposure(clients={"daemon_a": [("x.model", "do_it"), ("x.model", "more")], "cert:CN=a": [("x.model", "do_it")]})
        allowed = self._call("service", login="daemon_a", cert="cert:CN=a")
        self.assertEqual(policy.decide(allowed, both)[0], ALLOW)
        # In the login's list but not the certificate's: refused.
        narrower = self._call("service", login="daemon_a", cert="cert:CN=a", method="more")
        self.assertEqual(policy.decide(narrower, both), (DENY, "client-not-listed"))
        # Another daemon's certificate cannot widen a login that has its own entry.
        widened = self._call("service", login="daemon_b", cert="cert:CN=a", method="do_it")
        self.assertEqual(policy.decide(widened, _exposure(clients={"daemon_b": [("y.model", "z")], "cert:CN=a": [("x.model", "do_it")]}))[0], DENY)
        # A certificate with no entry changes nothing.
        self.assertEqual(policy.decide(self._call("service", login="daemon_a", cert="cert:CN=unknown"), both)[0], ALLOW)

    def test_a_client_mark_admits_that_login(self):
        marked = self._call("service", login="daemon_a", marks=({"portal": False, "clients": ("daemon_a",)},))
        self.assertEqual(policy.decide(marked, _exposure(clients={"daemon_a": []}))[0], ALLOW)


@tagged("post_install", "-at_install")
class TestModesAndCounters(HamsTransactionCase):
    """Modes, the parameter readers and the report-only counters."""
    # Tests [@ANCHOR: hams_rpc_guard:normalise_mode]
    # Tests [@ANCHOR: hams_rpc_guard:truthy]
    # Tests [@ANCHOR: hams_rpc_guard:parse_authors]
    # Tests [@ANCHOR: hams_rpc_guard:author_is_managed]
    # Tests [@ANCHOR: hams_rpc_guard:record_verdict]
    # Tests [@ANCHOR: hams_rpc_guard:verdict_counts]
    # Tests [@ANCHOR: hams_rpc_guard:apply_verdict]

    def setUp(self):
        super().setUp()
        policy._counts.clear()
        self.addCleanup(policy._counts.clear)
        self.call = Call(model="x.model", method="do_it", user_class="portal")

    def test_mode_defaults_and_fails_closed(self):
        self.assertEqual(policy.normalise_mode(None), "enforce")
        self.assertEqual(policy.normalise_mode(" Report "), "report")
        self.assertEqual(policy.normalise_mode("off"), "off")
        self.assertEqual(policy.normalise_mode("sure"), "enforce")

    def test_truthy_and_authors(self):
        self.assertTrue(policy.truthy("Yes"))
        self.assertTrue(policy.truthy("1"))
        self.assertFalse(policy.truthy(""))
        self.assertFalse(policy.truthy("no"))
        self.assertEqual(policy.parse_authors(""), policy.DEFAULT_MANAGED_AUTHORS)
        self.assertEqual(policy.parse_authors(" Acme , Foo "), ("Acme", "Foo"))
        self.assertTrue(policy.author_is_managed("Bruce Perens K6BP", policy.DEFAULT_MANAGED_AUTHORS))
        self.assertTrue(policy.author_is_managed("the hams team", policy.DEFAULT_MANAGED_AUTHORS))
        self.assertFalse(policy.author_is_managed("Odoo S.A.", policy.DEFAULT_MANAGED_AUTHORS))
        self.assertFalse(policy.author_is_managed(None, policy.DEFAULT_MANAGED_AUTHORS))

    def test_report_mode_counts_and_serves(self):
        with self.assertLogs(policy._logger, "WARNING") as logs:
            policy.apply_verdict(self.call, DENY, "strict-portal", "report")  # no exception
        self.assertEqual(policy.verdict_counts(), {("report", "portal", "x.model.do_it", "strict-portal"): 1})
        self.assertIn("report-only", logs.output[0])

    def test_enforce_mode_refuses_with_an_access_error_that_names_only_the_pair(self):
        with self.assertLogs(policy._logger, "WARNING"), self.assertRaises(AccessError) as caught:
            policy.apply_verdict(self.call, DENY, "strict-portal", "enforce")
        self.assertEqual(str(caught.exception), "The method 'x.model.do_it' cannot be called remotely.")

    def test_allowed_calls_are_not_counted_and_the_log_is_at_powers_of_ten(self):
        policy.apply_verdict(self.call, ALLOW, "core", "enforce")
        self.assertEqual(policy.verdict_counts(), {})
        with self.assertLogs(policy._logger, "WARNING") as logs:
            for _ in range(11):
                policy.record_verdict(self.call, DENY, "strict-portal", "report")
        self.assertEqual(len(logs.records), 2, "the 1st and the 10th")
        for line in logs.output:
            self.assertNotIn("password", line.lower())
