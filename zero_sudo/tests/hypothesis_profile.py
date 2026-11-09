# -*- coding: utf-8 -*-
# Part of Odoo. See LICENSE file for full copyright and licensing details.
#
# This file is part of hams_open, an open source module.
# SPDX-License-Identifier: AGPL-3.0-or-later
"""The one Hypothesis settings profile of every property test that runs inside an Odoo test process.

`zero_sudo.tests.common` calls `register_hams_profile()` when it is imported, so every test class built on the
harness has the profile, and a property test that does not use the harness (edge_routing's) calls it itself. Why the profile exists, and what it changes, is in `zero_sudo/docs/stories/testing_facility.md`."""
from hypothesis import HealthCheck, settings

PROFILE_NAME = "hams_odoo_tests"


# [@ANCHOR: zero_sudo:hypothesis_profile]
# Verified by [@ANCHOR: zero_sudo:test_hypothesis_profile_is_load_proof_and_keeps_other_checks]
def register_hams_profile():
    """Register and load the `hams_odoo_tests` profile; calling it again is harmless (it re-registers the same values)."""
    settings.register_profile(
        PROFILE_NAME,
        deadline=None,
        derandomize=True,
        max_examples=200,
        suppress_health_check=[HealthCheck.too_slow],
    )
    settings.load_profile(PROFILE_NAME)

