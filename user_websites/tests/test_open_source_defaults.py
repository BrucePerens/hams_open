# Copyright © Bruce Perens K6BP.
# SPDX-License-Identifier: AGPL-3.0-or-later

# -*- coding: utf-8 -*-
"""The test-package pinning helpers in open_source_defaults.py, called directly."""
import sys
import types
import unittest

import odoo.sql_db
from odoo.tests import tagged
from odoo.tests.common import get_db_name

from odoo.addons.user_websites.tests import open_source_defaults as defaults
from odoo.addons.zero_sudo.tests.common import HamsTransactionCase

KEY = "user_websites.test_open_source_defaults_probe"


def _read_committed(key):
    with odoo.sql_db.db_connect(get_db_name()).cursor() as cr:
        cr.execute("SELECT value FROM ir_config_parameter WHERE key = %s", (key,))  # audit-ignore-sql: test fixture, parameterised
        row = cr.fetchone()
    return row[0] if row else None


@tagged("post_install", "-at_install")
class TestOpenSourceDefaults(HamsTransactionCase):

    def test_01_set_committed_param_upserts_deletes_and_returns_the_old_value(self):
        # Tests [@ANCHOR: user_websites:fn_open_source_set_committed_param]
        self.addCleanup(defaults._set_committed_param, KEY, None)
        self.assertIsNone(defaults._set_committed_param(KEY, "one"))
        self.assertEqual(_read_committed(KEY), "one")
        self.assertEqual(defaults._set_committed_param(KEY, "two"), "one")
        self.assertEqual(_read_committed(KEY), "two")
        self.assertEqual(defaults._set_committed_param(KEY, None), "two")
        self.assertIsNone(_read_committed(KEY))
        self.assertIsNone(defaults._set_committed_param(KEY, None), "deleting a missing row is harmless")

    def test_02_the_open_source_gaps_are_empty_whatever_a_deployment_overrides(self):
        # Tests [@ANCHOR: user_websites:fn_open_source_eligibility_gaps]
        self.assertEqual(defaults._open_source_eligibility_gaps(self.env.user), [])

    def test_03_wrap_pins_the_policy_and_the_gaps_for_the_class_and_restores_them(self):
        # Tests [@ANCHOR: user_websites:fn_open_source_wrap]
        shield_key = defaults._FORM_SHIELD_TUNING_KEYS[0]
        self.addCleanup(defaults._set_committed_param, shield_key, defaults._set_committed_param(shield_key, "99"))
        self.addCleanup(defaults._set_committed_param, defaults._SLUG_POLICY_KEY, defaults._set_committed_param(defaults._SLUG_POLICY_KEY, "callsign"))
        reg = self.registry
        seen = {}

        class Probe(unittest.TestCase):
            registry = reg

            @classmethod
            def setUpClass(cls):
                seen["policy"] = _read_committed(defaults._SLUG_POLICY_KEY)
                seen["shield"] = _read_committed(shield_key)
                seen["gaps"] = cls.registry["res.users"]._website_eligibility_gaps

        defaults._wrap(Probe)
        Probe.setUpClass()
        self.assertEqual(seen["policy"], "name_callsign")
        self.assertIsNone(seen["shield"])
        self.assertIs(seen["gaps"], defaults._open_source_eligibility_gaps)
        Probe.doClassCleanups()
        self.assertEqual(_read_committed(defaults._SLUG_POLICY_KEY), "callsign")
        self.assertEqual(_read_committed(shield_key), "99")

    def test_04_pin_package_wraps_the_database_test_cases_of_the_named_package_only(self):
        # Tests [@ANCHOR: user_websites:fn_open_source_pin_package]
        test_module = types.ModuleType("probe_pkg.test_x")
        helper_module = types.ModuleType("probe_pkg.helper")

        class Pinned(HamsTransactionCase):
            pass

        class Plain:
            pass

        class Elsewhere(HamsTransactionCase):
            pass

        Pinned.__module__ = Plain.__module__ = "probe_pkg.test_x"
        Elsewhere.__module__ = "probe_pkg.helper"
        test_module.Pinned, test_module.Plain = Pinned, Plain
        helper_module.Elsewhere = Elsewhere
        for name, module in (("probe_pkg.test_x", test_module), ("probe_pkg.helper", helper_module)):
            sys.modules[name] = module
            self.addCleanup(sys.modules.pop, name, None)
        defaults.pin_package("probe_pkg")
        self.assertTrue(Pinned.__dict__.get("_open_source_pinned"))
        self.assertNotIn("_open_source_pinned", Plain.__dict__)
        self.assertNotIn("_open_source_pinned", Elsewhere.__dict__, "only the test_ modules are wrapped")
        wrapped = Pinned.__dict__["setUpClass"]
        defaults.pin_package("probe_pkg")
        self.assertIs(Pinned.__dict__["setUpClass"], wrapped, "a class is wrapped once")
