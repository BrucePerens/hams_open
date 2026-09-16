# -*- coding: utf-8 -*-
# This software is distributed under the terms of the Affero General Public License (AGPL-3).
# SPDX-License-Identifier: AGPL-3.0-or-later
"""Proves HamsHttpCase.browser_js() refuses to pass a hoot run that executed
no tests.

This is the discriminating pair for the guard. Both halves are required: the
first shows an empty run now fails, the second shows a deliberate empty run
can still be declared. A guard with only the first test cannot be
distinguished from a harness that is simply broken, and one with only the
second proves nothing at all.

Why a test and not just the checker: check_hoot_runner_coverage.py catches
the three STATIC shapes (a .test.js no bundle lists, a bundled suite whose
tag no wrapper requests, a describe with no test()). It cannot catch a suite
whose tests are all skipped at runtime, nor any future shape that yields an
empty result for a reason nobody has thought of. The checker proves a
wrapper's tag CAN match tests; only the run proves it DID.
"""
from odoo.addons.zero_sudo.tests.common import HamsHttpCase
from odoo.tests.common import tagged


# A tag no describe() in either repository declares. The run therefore loads
# /web/tests, registers zero jobs, and reports "Passed 0 tests" followed by
# "Test suite succeeded" -- the exact false-green this guard exists to catch.
_TAG_THAT_MATCHES_NOTHING = "zero_sudo_deliberately_nonexistent_tag_for_guard_test"

_EMPTY_RUN_URL = (
    "/web/tests?headless&loglevel=2&preset=desktop&timeout=15000&tag="
    + _TAG_THAT_MATCHES_NOTHING
)


@tagged("post_install", "-at_install")
class TestHootEmptyRunGuard(HamsHttpCase):
    # Tests [@ANCHOR: zero_sudo:hams_http_case_browser_js]
    # Tests [@ANCHOR: zero_sudo:hoot_empty_run_detector_emit]
    def test_hoot_guard_fails_a_suite_that_runs_no_tests(self):
        """A hoot run that executes zero tests must fail, not pass silently."""
        with self.assertRaises(AssertionError) as caught:
            self.browser_js(
                _EMPTY_RUN_URL,
                "",
                "",
                login="admin",
                timeout=120,
                success_signal="[HOOT] Test suite succeeded",
            )
        self.assertIn(
            "executed ZERO tests",
            str(caught.exception),
            "[!] DIAGNOSTIC FOR AI: browser_js() raised, but not with the "
            "empty-run guard's own message. Some other failure produced this "
            "AssertionError, so this test is not proving what it claims. Read "
            "the exception text before assuming the guard works.",
        )

    def test_expect_empty_allows_a_deliberately_empty_run(self):
        """expect_empty=True is the documented, deliberate escape hatch."""
        self.browser_js(
            _EMPTY_RUN_URL,
            "",
            "",
            login="admin",
            timeout=120,
            success_signal="[HOOT] Test suite succeeded",
            expect_empty=True,
        )
