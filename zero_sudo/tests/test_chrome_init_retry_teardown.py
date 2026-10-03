# -*- coding: utf-8 -*-
# SPDX-License-Identifier: AGPL-3.0-or-later
"""A failed headless-Chrome start must be torn down before the retry, not orphaned by it.

Core's ChromeBrowser.__init__ registers its resources in `self.cleanup` and has no error handling; the retry wrapper used
to run it again on the same object, replacing `cleanup` and leaving the first attempt's receiver thread to delete the
second attempt's websocket. These tests drive the wrapper with a fake constructor (no Chrome needed)."""
import os
from contextlib import ExitStack
from unittest.mock import patch

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
        # The fake browser has no DevTools session to install the navigator overrides into.
        self.safe_patch_object(common, "_install_navigator_overrides", lambda browser: None)
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

    # Tests [@ANCHOR: zero_sudo:patched_chrome_init]
    def test_pause_on_fail_pins_only_the_instance_not_the_class(self):
        """night_shift_todo/low/zero-sudo-pause-on-fail-pins-cdp-port-9222-process-wide-624bd3a2.md.

        HAMS_PAUSE_ON_FAIL=1 must pin remote_debugging_port on the one browser built while it's
        set, never on ChromeBrowser itself -- a class-attribute write used to leak the pin onto
        every later browser built in the same process, including ones built after the var is
        unset again."""
        self.safe_patch_object(common, "original_chrome_init", self._fake_init(fail_times=0))
        self.safe_patch_object(common, "_js_coverage_start", lambda browser: None)

        with patch.dict(os.environ, {"HAMS_PAUSE_ON_FAIL": "1"}):
            pinned_browser = _Browser()
            common._patched_chrome_init(pinned_browser)

        self.assertEqual(
            pinned_browser.remote_debugging_port,
            9222,
            "the browser built while the env var is set must get the known, fixed port",
        )
        self.assertNotIn(
            "remote_debugging_port",
            vars(_Browser),
            "the class itself must stay unpinned, or every later browser in the process "
            "would inherit port 9222 regardless of the env var",
        )

        later_browser = _Browser()
        common._patched_chrome_init(later_browser)
        self.assertNotIn(
            "remote_debugging_port",
            vars(later_browser),
            "a browser built after the env var is unset again must not inherit the earlier pin",
        )
