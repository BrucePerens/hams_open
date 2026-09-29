# -*- coding: utf-8 -*-
# This software is distributed under the terms of the Affero General Public License (AGPL-3).
# SPDX-License-Identifier: AGPL-3.0-or-later
"""A failed headless-Chrome start must be torn down before the retry, not orphaned by it.

Core's ChromeBrowser.__init__ registers its resources in `self.cleanup` and has no error handling; the retry wrapper used
to run it again on the same object, replacing `cleanup` and leaving the first attempt's receiver thread to delete the
second attempt's websocket. These tests drive the wrapper with a fake constructor (no Chrome needed)."""
from contextlib import ExitStack

from odoo.addons.zero_sudo.tests import common
from odoo.addons.zero_sudo.tests.common import HamsTransactionCase
from odoo.tests.common import tagged


class _Browser:
    pass


@tagged("post_install", "-at_install")
class TestChromeInitRetryTeardown(HamsTransactionCase):
    def setUp(self):
        super().setUp()
        self.safe_patch("odoo.addons.zero_sudo.tests.common.time.sleep")
        self.events = []

    def _fake_init(self, fail_times):
        attempts = {"n": 0}

        def fake(browser, *args, **kwargs):
            attempts["n"] += 1
            browser.cleanup = ExitStack()
            browser.cleanup.callback(self.events.append, f"closed attempt {attempts['n']}")
            browser.ws = f"socket {attempts['n']}"
            if attempts["n"] <= fail_times:
                raise RuntimeError(f"late failure {attempts['n']}")

        return fake

    # Tests [@ANCHOR: zero_sudo:tear_down_partly_built_browser]
    def test_a_failed_attempt_is_torn_down_before_the_next_one_starts(self):
        self.safe_patch_object(common, "original_chrome_init", self._fake_init(fail_times=1))
        self.safe_patch_object(common, "_js_coverage_start", lambda browser: None)
        browser = _Browser()
        common._patched_chrome_init(browser)
        self.assertEqual(self.events, ["closed attempt 1"], "attempt 1's resources must be closed, attempt 2's kept open")
        self.assertEqual(browser.ws, "socket 2", "the succeeding attempt's websocket must be the one left on the object")

    def test_every_attempt_is_torn_down_when_all_of_them_fail(self):
        self.safe_patch_object(common, "original_chrome_init", self._fake_init(fail_times=3))
        browser = _Browser()
        with self.assertRaises(RuntimeError):
            common._patched_chrome_init(browser)
        self.assertEqual(self.events, ["closed attempt 1", "closed attempt 2", "closed attempt 3"])
        self.assertFalse(vars(browser).get("ws"))
