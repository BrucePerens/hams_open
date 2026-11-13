# -*- coding: utf-8 -*-
# Copyright © Bruce Perens K6BP.
# SPDX-License-Identifier: AGPL-3.0-or-later
"""Tests added to kill the surviving mutants of policy.py (mutation run 2026-11-13). Each test names what it pins."""
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
class TestPolicyMutants(HamsTransactionCase):
    # Tests [@ANCHOR: hams_rpc_guard:decide]
    # Tests [@ANCHOR: hams_rpc_guard:call_init]
    # Tests [@ANCHOR: hams_rpc_guard:truthy]
    # Tests [@ANCHOR: hams_rpc_guard:record_verdict]

    PAIR = ("x.model", "do_it")

    def test_call_defaults_are_the_strict_ones(self):
        """A Call built with only the required facts is not a standard ORM method and not a service account."""
        call = Call("x.model", "do_it", "internal", managed=True)
        self.assertFalse(call.orm_standard)
        self.assertFalse(Call("x.model", "do_it", "internal").managed)
        self.assertEqual((call.login, call.cert, call.defining_models, call.marks), ("", "", (), ()))
        self.assertEqual(policy.decide(call, _exposure()), (DENY, "managed-not-listed"))

    def test_every_verdict_carries_its_reason(self):
        """The reason strings feed the counters and the log, so each rule's own reason is part of the contract."""
        def call(user_class, **kw):
            values = dict(model="x.model", method="do_it", defining_models=("x.model",), managed=True)
            values.update(kw)
            return Call(user_class=user_class, **values)
        pair = [self.PAIR]
        self.assertEqual(policy.decide(call("portal"), _exposure(portal=pair)), (ALLOW, "portal-listed"))
        self.assertEqual(policy.decide(call("portal"), _exposure()), (DENY, "strict-portal"))
        self.assertEqual(policy.decide(call("public"), _exposure()), (DENY, "strict-public"))
        self.assertEqual(policy.decide(call("service", login="d"), _exposure(clients={"d": pair})),
                         (ALLOW, "client-listed"))
        self.assertEqual(policy.decide(call("service", login="d"), _exposure(clients={"d": [("a", "b")]})),
                         (DENY, "client-not-listed"))
        self.assertEqual(policy.decide(call("internal"), _exposure(internal=pair)), (ALLOW, "listed"))
        self.assertEqual(policy.decide(call("service", login="u"), _exposure(internal=pair)),
                         (ALLOW, "unmapped-client-listed"))
        self.assertEqual(policy.decide(call("service", login="u"), _exposure()),
                         (DENY, "unmapped-client-managed-not-listed"))
        stock = call("internal", managed=False)
        self.assertEqual(policy.decide(stock, _exposure(stock=pair), strict_stock=True), (ALLOW, "stock-listed"))
        self.assertEqual(policy.decide(stock, _exposure(), strict_stock=False), (ALLOW, "core"))

    def test_a_client_entry_only_binds_service_accounts(self):
        """An internal user whose login happens to be a daemon login is judged by the internal rules, not the daemon's list."""
        clients = {"d": [("a", "b")]}
        call = Call("x.model", "do_it", "internal", login="d", defining_models=("x.model",), managed=True)
        self.assertEqual(policy.decide(call, _exposure(internal=[self.PAIR], clients=clients)), (ALLOW, "listed"))
        cert = Call("x.model", "do_it", "internal", cert="cert:d", defining_models=("x.model",), managed=True)
        self.assertEqual(policy.decide(cert, _exposure(internal=[self.PAIR], clients={"cert:d": [("a", "b")]})),
                         (ALLOW, "listed"))

    def test_truthy_accepts_exactly_the_documented_words(self):
        for word in ("true", "TRUE", " on ", "On", "yes", "1"):
            self.assertTrue(policy.truthy(word), word)
        for word in ("false", "off", "0", "", None, "2", "ye", "truee"):
            self.assertFalse(policy.truthy(word), word)

    def test_record_verdict_returns_the_count_and_zero_for_allowed(self):
        policy._counts.clear()
        self.addCleanup(policy._counts.clear)
        call = Call("x.model", "do_it", "portal")
        self.assertEqual(policy.record_verdict(call, ALLOW, "core", "enforce"), 0)
        self.assertEqual(policy.record_verdict(call, DENY, "strict-portal", "enforce"), 1)
        self.assertEqual(policy.record_verdict(call, DENY, "strict-portal", "enforce"), 2)
        self.assertEqual(policy.record_verdict(call, DENY, "strict-portal", "report"), 1, "mode is part of the key")

    def test_only_report_mode_says_the_call_was_served(self):
        policy._counts.clear()
        self.addCleanup(policy._counts.clear)
        call = Call("x.model", "do_it", "portal")
        with self.assertLogs(policy._logger, "WARNING") as logs:
            policy.record_verdict(call, DENY, "strict-portal", "enforce")
        self.assertTrue(logs.output[0].endswith("reason strict-portal."))
        policy._counts.clear()
        with self.assertLogs(policy._logger, "WARNING") as logs:
            policy.record_verdict(call, DENY, "strict-portal", "report")
        self.assertTrue(logs.output[0].endswith("reason strict-portal. The call was served: report-only mode."))
