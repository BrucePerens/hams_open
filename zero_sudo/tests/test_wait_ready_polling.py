# -*- coding: utf-8 -*-
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
        if len(self.calls) > 1000:
            raise AssertionError("_wait_ready is spinning: over 1000 evaluate calls")
        self.calls.append((method, timeout))
        nxt = self._responses[0] if len(self._responses) == 1 else self._responses.pop(0)
        if isinstance(nxt, BaseException):
            raise nxt
        return {"result": nxt}

    def take_screenshot(self, prefix=None):
        self.screenshots.append(prefix)


class _FakeClock:
    """An injected clock: sleep() advances it, nothing else does, so the polling is independent of real time and box load."""

    def __init__(self):
        self.now = 1000.0
        self.sleeps = []

    def time(self):
        return self.now

    def sleep(self, seconds):
        if len(self.sleeps) > 1000:
            raise AssertionError("_wait_ready is spinning: over 1000 sleeps on the fake clock")
        self.sleeps.append(seconds)
        # Always move at least a microsecond: a sub-ulp sleep at 1000.0 would not change `now` at all.
        self.now += max(seconds, 1e-6)

    def __getattr__(self, name):
        return getattr(time, name)


@tagged("post_install", "-at_install")
class TestWaitReadyPolling(HamsTransactionCase):
    # Tests [@ANCHOR: zero_sudo:patched_wait_ready]
    def _fake_clock(self):
        clock = _FakeClock()
        self.safe_patch_object(common, "time", clock)
        return clock

    def test_installed_on_chrome_browser(self):
        self.assertIs(ChromeBrowser._wait_ready, common._patched_wait_ready)

    def test_success_path_returns_true_immediately(self):
        browser = _FakeBrowser([_TRUE])
        clock = self._fake_clock()
        self.assertTrue(common._patched_wait_ready(browser, "1 === 1", timeout=60))
        self.assertEqual(clock.sleeps, [], "a ready page is not waited on")
        self.assertEqual(len(browser.calls), 1)
        self.assertEqual(browser.screenshots, [])

    def test_falsy_ready_code_does_not_busy_loop(self):
        browser = _FakeBrowser([_FALSE])
        clock = self._fake_clock()
        self.assertFalse(common._patched_wait_ready(browser, "false", timeout=1))
        self.assertGreaterEqual(sum(clock.sleeps), 0.99, "it polls until the deadline, on the injected clock")
        # ~0.1s poll interval: about 10 calls a second, never thousands.
        self.assertLessEqual(len(browser.calls), 15, browser.calls[:5])
        self.assertEqual(browser.screenshots, ["sc_failed_ready_"])

    def test_per_call_timeout_is_retried(self):
        browser = _FakeBrowser(
            [TimeoutError("Runtime.evaluate(x)"), TimeoutError("y"), _TRUE]
        )
        self._fake_clock()
        self.assertTrue(common._patched_wait_ready(browser, "x", timeout=5))
        self.assertEqual(len(browser.calls), 3)

    def test_overall_deadline_still_fails_when_calls_always_time_out(self):
        browser = _FakeBrowser([TimeoutError("Runtime.evaluate(x)")])
        clock = self._fake_clock()
        # Core returns False (browser_js then fails with 'ready code was
        # always falsy'); a TimeoutError must not escape.
        self.assertFalse(common._patched_wait_ready(browser, "x", timeout=1))
        self.assertGreaterEqual(sum(clock.sleeps), 0.99)
        self.assertLessEqual(len(browser.calls), 15)
        self.assertEqual(browser.screenshots, ["sc_failed_ready_"])

    def test_stored_websocket_exception_is_raised_like_core(self):
        browser = _FakeBrowser([_FALSE])
        browser._result.set_exception(RuntimeError("ws died"))
        self._fake_clock()
        with self.assertRaisesRegex(RuntimeError, "ws died"):
            common._patched_wait_ready(browser, "false", timeout=0.3)

    def test_long_ready_code_is_truncated_in_log_only(self):
        browser = _FakeBrowser([_TRUE])
        code = "1 === 1 || " + "x" * 300
        with self.assertLogs("zero_sudo.fake_browser", "INFO") as logs:
            common._patched_wait_ready(browser, code, timeout=5)
        self.assertIn(" ...", logs.output[0])
        self.assertNotIn("x" * 200, logs.output[0])
