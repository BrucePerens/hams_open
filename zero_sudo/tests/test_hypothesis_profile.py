# -*- coding: utf-8 -*-
# Part of Odoo. See LICENSE file for full copyright and licensing details.
#
# This file is part of hams_open, an open source module.
# SPDX-License-Identifier: AGPL-3.0-or-later
"""The Hypothesis profile shared by every Odoo-run property test (zero_sudo/tests/hypothesis_profile.py)."""
from hypothesis import HealthCheck, given, settings, strategies as st

from odoo.tests.common import BaseCase, tagged

from odoo.addons.zero_sudo.tests import hypothesis_profile


@tagged("post_install", "-at_install")
class TestHypothesisProfile(BaseCase):
    def test_hypothesis_profile_is_load_proof_and_keeps_other_checks(self):
        # Tests [@ANCHOR: zero_sudo:hypothesis_profile]
        # Tests [@ANCHOR: zero_sudo:test_hypothesis_profile_is_load_proof_and_keeps_other_checks]
        hypothesis_profile.register_hams_profile()
        current = settings()
        self.assertIsNone(current.deadline, "a loaded machine must not fail an example for being slow")
        self.assertTrue(current.derandomize, "the examples must be the same on every run")
        self.assertEqual(current.max_examples, 200)
        # Only too_slow is suppressed: every other health check stays on.
        self.assertEqual(list(current.suppress_health_check), [HealthCheck.too_slow])
        # A settings object a test file builds itself inherits the profile for what it does not name.
        own = settings(max_examples=50)
        self.assertEqual(own.max_examples, 50)
        self.assertIsNone(own.deadline)
        self.assertTrue(own.derandomize)
        self.assertEqual(list(own.suppress_health_check), [HealthCheck.too_slow])
        # A property still runs under it, and a failing one still fails.
        seen = []

        @given(st.integers())
        def holds(value):
            seen.append(value)
            self.assertEqual(value - value, 0)

        holds()
        self.assertGreater(len(seen), 1)

        @given(st.integers())
        def fails(value):
            self.assertLess(value, 0)

        with self.assertRaises(AssertionError):
            fails()
