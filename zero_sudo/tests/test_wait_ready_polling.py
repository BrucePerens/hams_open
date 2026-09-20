# -*- coding: utf-8 -*-
# This software is distributed under the terms of the Affero General Public License (AGPL-3).
# SPDX-License-Identifier: AGPL-3.0-or-later
"""ChromeBrowser._wait_ready must poll politely and survive per-call timeouts.

Odoo core's version busy-loops ~110k Runtime.evaluate calls per minute and
lets the last call's ~0s timeout escape as a bare TimeoutError. These tests
drive our replacement with a fake browser object (no Chrome needed).
"""
import logging
import time
from concurrent.futures import Future

from odoo.addons.zero_sudo.tests import common
from odoo.addons.zero_sudo.tests.common import HamsTransactionCase
from odoo.tests.common import ChromeBrowser, tagged

_TRUE = {"type": "boolean", "value": True}
_FALSE = {"type": "boolean", "value": False}


class _FakeBrowser:
    """Just the attributes _wait_ready touches."""

    throttling_factor = 1

    def __init__(self, responses):
        self._responses = list(responses)
        self._logger = logging.getLogger("zero_sudo.fake_browser")
        self._result = Future()
        self.calls = []
        self.screenshots = []

    def _websocket_request(self, method, *, params=None, timeout=10.0):
        self.calls.append((method, timeout))
        nxt = self._responses[0] if len(self._responses) == 1 else self._responses.pop(0)
        if isinstance(nxt, BaseException):
            raise nxt
        return {"result": nxt}

    def take_screenshot(self, prefix=None):
        self.screenshots.append(prefix)


@tagged("post_install", "-at_install")
class TestWaitReadyPolling(HamsTransactionCase):
    # Tests [@ANCHOR: zero_sudo:patched_wait_ready]
    def test_installed_on_chrome_browser(self):
        self.assertIs(ChromeBrowser._wait_ready, common._patched_wait_ready)

    def test_success_path_returns_true_immediately(self):
        browser = _FakeBrowser([_TRUE])
        start = time.time()
        self.assertTrue(common._patched_wait_ready(browser, "1 === 1", timeout=5))
        self.assertLess(time.time() - start, 1.0)
        self.assertEqual(len(browser.calls), 1)
        self.assertEqual(browser.screenshots, [])

    def test_falsy_ready_code_does_not_busy_loop(self):
        browser = _FakeBrowser([_FALSE])
        start = time.time()
        self.assertFalse(common._patched_wait_ready(browser, "false", timeout=1))
        elapsed = time.time() - start
        self.assertGreaterEqual(elapsed, 0.9)
        # ~0.1s poll interval: about 10 calls a second, never thousands.
        self.assertLessEqual(len(browser.calls), 15, browser.calls[:5])
        self.assertEqual(browser.screenshots, ["sc_failed_ready_"])

    def test_per_call_timeout_is_retried(self):
        browser = _FakeBrowser(
            [TimeoutError("Runtime.evaluate(x)"), TimeoutError("y"), _TRUE]
        )
        self.assertTrue(common._patched_wait_ready(browser, "x", timeout=5))
        self.assertEqual(len(browser.calls), 3)

    def test_overall_deadline_still_fails_when_calls_always_time_out(self):
        browser = _FakeBrowser([TimeoutError("Runtime.evaluate(x)")])
        start = time.time()
        # Core returns False (browser_js then fails with 'ready code was
        # always falsy'); a TimeoutError must not escape.
        self.assertFalse(common._patched_wait_ready(browser, "x", timeout=1))
        self.assertGreaterEqual(time.time() - start, 0.9)
        self.assertLessEqual(len(browser.calls), 15)
        self.assertEqual(browser.screenshots, ["sc_failed_ready_"])

    def test_stored_websocket_exception_is_raised_like_core(self):
        browser = _FakeBrowser([_FALSE])
        browser._result.set_exception(RuntimeError("ws died"))
        with self.assertRaisesRegex(RuntimeError, "ws died"):
            common._patched_wait_ready(browser, "false", timeout=0.3)

    def test_long_ready_code_is_truncated_in_log_only(self):
        browser = _FakeBrowser([_TRUE])
        code = "1 === 1 || " + "x" * 300
        with self.assertLogs("zero_sudo.fake_browser", "INFO") as logs:
            common._patched_wait_ready(browser, code, timeout=5)
        self.assertIn(" ...", logs.output[0])
        self.assertNotIn("x" * 200, logs.output[0])
