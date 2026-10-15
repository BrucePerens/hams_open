# Copyright © Bruce Perens K6BP.
# SPDX-License-Identifier: AGPL-3.0-or-later

# -*- coding: utf-8 -*-
"""Make this module's own tests behave the same whatever hams.com modules share the database.

hams.com layers two things on user_websites: ham_onboarding's data sets `user_websites.slug_policy`
to "callsign", and ham_communications_consent overrides `ResUsers._website_eligibility_gaps()` so a
member who is not a verified ham with a signed agreement has no site (the site 404s). Both are
correct for hams.com and wrong for these tests, which exercise the open-source behaviour with plain
members. Without this, running `user_websites` in the same `test.py -u` run as the hams.com modules
fails dozens of tests.

pin_package() wraps `setUpClass` of every database test case defined in this package so that, for the
life of the class:
  * the slug policy is the open-source default (a committed value, restored afterwards, so tests that
    run real committed transactions and the HTTP worker threads see it too), and
  * `_website_eligibility_gaps()` is the open-source implementation, bypassing every hams.com
    override. A test that patches ResUsersSiteEligibility._website_eligibility_gaps itself (the
    extension-point test) still takes effect, because the open-source implementation is looked up at
    call time.

Nothing here touches the hams.com modules' own tests: they live in other packages and keep the real rule.
"""

import sys
from unittest import mock

from odoo.tests.common import HttpCase, TransactionCase, get_db_name
import odoo.sql_db

from odoo.addons.user_websites.models.site_eligibility import ResUsersSiteEligibility

_SLUG_POLICY_KEY = "user_websites.slug_policy"
_OPEN_SOURCE_POLICY = "name_callsign"


def _set_committed_param(key, value):
    """Upsert (or, for value None, delete) an ir_config_parameter row on its own committed connection and return the old value."""
    with odoo.sql_db.db_connect(get_db_name()).cursor() as cr:
        cr.execute("SELECT value FROM ir_config_parameter WHERE key = %s", (key,))  # audit-ignore-sql: test fixture, parameterised
        row = cr.fetchone()
        if value is None:
            cr.execute("DELETE FROM ir_config_parameter WHERE key = %s", (key,))  # audit-ignore-sql: test fixture, parameterised
        else:
            cr.execute(  # audit-ignore-sql: test fixture, parameterised
                "INSERT INTO ir_config_parameter (key, value) VALUES (%s, %s) ON CONFLICT (key) DO UPDATE SET value = EXCLUDED.value",
                (key, value),
            )
    return row[0] if row else None


def _open_source_eligibility_gaps(self):
    return ResUsersSiteEligibility._website_eligibility_gaps(self)


def _wrap(cls):
    own = cls.__dict__.get("setUpClass")
    inner = own.__func__ if own is not None else super(cls, cls).setUpClass.__func__

    def setUpClass(c):
        previous = _set_committed_param(_SLUG_POLICY_KEY, _OPEN_SOURCE_POLICY)
        # Registered before the inner setUpClass so it runs after Odoo's own rollback/close cleanups
        # (LIFO): restoring on another connection must not wait on a row lock the class still holds.
        c.addClassCleanup(_set_committed_param, _SLUG_POLICY_KEY, previous)
        inner(c)
        patcher = mock.patch.object(c.registry["res.users"], "_website_eligibility_gaps", _open_source_eligibility_gaps)
        patcher.start()
        c.addClassCleanup(patcher.stop)

    cls.setUpClass = classmethod(setUpClass)


def pin_package(package_name):
    """Wrap every database test case defined in the test modules of `package_name` (call after the imports)."""
    for name, module in list(sys.modules.items()):
        if module is None or not name.startswith(package_name + ".test_"):
            continue
        for obj in vars(module).values():
            if (
                isinstance(obj, type)
                and obj.__module__ == name
                and issubclass(obj, (TransactionCase, HttpCase))
                and "_open_source_pinned" not in obj.__dict__
            ):
                _wrap(obj)
                obj._open_source_pinned = True
