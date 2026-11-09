# -*- coding: utf-8 -*-
# SPDX-License-Identifier: AGPL-3.0-or-later
"""The per-class Chrome pool (zero_sudo/tests/chrome_pool.py and its harness hooks in common.py).

Two kinds of test. The first drives `ChromePool` with a fake `start`, so reuse, replacement and release are proved without
a browser. The second (`TestPooledChromeIsolation`) uses the real headless Chrome of this test run: a page of one test must
see nothing of an earlier test's page, a crashed Chrome must fail only its own test, and the Chrome must survive the
end-of-test reaper and be reused by the next test of the class."""
import logging
import os
import tempfile
import time
from types import SimpleNamespace
from unittest.mock import patch

from odoo.addons.zero_sudo.tests import common
from odoo.addons.zero_sudo.tests.chrome_pool import ChromePool, ChromePoolError, PooledChrome
from odoo.addons.zero_sudo.tests.common import HamsHttpCase, HamsTransactionCase
from odoo.tests.common import ChromeBrowser, tagged

_logger = logging.getLogger(__name__)


class _FakeChrome:
    """A stand-in for the Chrome a pool's `start` returns: a Popen-like object that can die."""

    def __init__(self):
        self.returncode = None
        self.pid = os.getpid()

    def poll(self):
        return self.returncode


class _FakeStarter:
    def __init__(self):
        self.started = []
        self.stopped = []

    def __call__(self, touch_enabled):
        proc = _FakeChrome()
        profile = tempfile.mkdtemp(suffix="_fake_chrome_odoo")
        self.started.append((proc, touch_enabled))
        return proc, 9, profile, lambda: self.stopped.append(proc)


@tagged("post_install", "-at_install")
class TestChromePoolLogic(HamsTransactionCase):
    def setUp(self):
        super().setUp()
        self.starter = _FakeStarter()
        self.pool = ChromePool(self.starter, max_uses=3)
        self.addCleanup(self.pool.close)
        # The fake has no DevTools endpoint: liveness is the process state alone here.
        self.safe_patch_object(PooledChrome, "alive", lambda chrome: chrome.proc.poll() is None)

    # Tests [@ANCHOR: zero_sudo:chrome_pool_acquire]
    def test_one_class_keeps_one_chrome(self):
        owner = type("Owner", (), {})
        first = self.pool.acquire(owner, False)
        second = self.pool.acquire(owner, False)
        self.assertIs(first, second)
        self.assertEqual(len(self.starter.started), 1)
        self.assertEqual(second.uses, 2)

    # Tests [@ANCHOR: zero_sudo:chrome_pool_acquire]
    def test_another_class_other_switches_age_or_a_dead_chrome_replace_it(self):
        one, other = type("One", (), {}), type("Other", (), {})
        chrome = self.pool.acquire(one, False)
        self.assertIsNot(self.pool.acquire(other, False), chrome, "another test class must not inherit a Chrome")
        self.assertEqual(self.starter.stopped, [chrome.proc])

        touch = self.pool.acquire(other, True)
        self.assertTrue(touch.touch_enabled, "touch events are a launch switch: a Chrome without them cannot serve the test")
        self.assertEqual(len(self.starter.started), 3)

        touch.proc.returncode = -15  # the Chrome died between tests
        revived = self.pool.acquire(other, True)
        self.assertIsNot(revived, touch)
        self.assertEqual(len(self.starter.started), 4)

        self.pool.acquire(other, True)
        self.pool.acquire(other, True)  # the third lease: max_uses is 3
        aged = self.pool.acquire(other, True)
        self.assertEqual(aged.uses, 1, "a Chrome past max_uses is replaced by a fresh one")

    # Tests [@ANCHOR: zero_sudo:chrome_pool_release]
    def test_an_unclean_test_stops_the_chrome_and_a_clean_one_keeps_it(self):
        owner = type("Owner", (), {})
        chrome = self.pool.acquire(owner, False)
        self.pool.release(chrome, clean=True)
        self.assertIs(self.pool.chrome, chrome)
        self.assertEqual(self.starter.stopped, [])
        self.pool.release(chrome, clean=False)
        self.assertIsNone(self.pool.chrome)
        self.assertEqual(self.starter.stopped, [chrome.proc])
        self.assertIsNot(self.pool.acquire(owner, False), chrome, "the next test gets a fresh Chrome")

    # Tests [@ANCHOR: zero_sudo:chrome_pool_release]
    def test_releasing_a_chrome_that_was_already_replaced_does_not_stop_its_successor(self):
        owner = type("Owner", (), {})
        old = self.pool.acquire(owner, False)
        self.pool.close()
        new = self.pool.acquire(owner, False)
        self.pool.release(old, clean=False)
        self.assertIs(self.pool.chrome, new)

    # Tests [@ANCHOR: zero_sudo:chrome_pool]
    # Tests [@ANCHOR: zero_sudo:pooled_chrome]
    # Tests [@ANCHOR: zero_sudo:chrome_pool_owned_pids]
    # Tests [@ANCHOR: zero_sudo:chrome_pool_owns_profile]
    # Tests [@ANCHOR: zero_sudo:chrome_pool_close]
    def test_the_reapers_are_told_which_processes_belong_to_the_pool(self):
        self.assertEqual(self.pool.owned_pids(), set())
        chrome = self.pool.acquire(type("Owner", (), {}), False)
        self.assertIn(chrome.proc.pid, self.pool.owned_pids())
        self.assertTrue(self.pool.owns_profile(chrome.user_data_dir))
        self.assertFalse(self.pool.owns_profile(tempfile.gettempdir()))
        self.pool.close()
        self.assertEqual(self.pool.owned_pids(), set())

    # Tests [@ANCHOR: zero_sudo:pool_applies]
    def test_the_pool_is_off_for_visible_debug_pause_on_fail_and_offline_runs(self):
        browser = SimpleNamespace()
        self.assertTrue(common._pool_applies(browser, {"headless": True, "debug": False}))
        self.assertFalse(common._pool_applies(browser, {"headless": False, "debug": False}), "watch mode shows a window")
        self.assertFalse(common._pool_applies(browser, {"headless": True, "debug": "assets"}))
        self.assertFalse(common._pool_applies(SimpleNamespace(_hams_unpooled=True), {}), "the pool's own start-up")
        for name, value in (("HAMS_CHROME_POOL", "0"), ("HAMS_PAUSE_ON_FAIL", "1"), ("HAMS_OFFLINE_NS_CTL", "/run/x")):
            with patch.dict(os.environ, {name: value}):
                self.assertFalse(common._pool_applies(browser, {}), name)

    # Tests [@ANCHOR: zero_sudo:pooled_test_was_clean]
    def test_a_test_is_clean_only_without_failure_signs(self):
        from concurrent.futures import Future

        self.assertTrue(common._pooled_test_was_clean(SimpleNamespace()))
        self.assertFalse(common._pooled_test_was_clean(SimpleNamespace(_hams_page_ok=False)))
        self.assertFalse(common._pooled_test_was_clean(SimpleNamespace(had_failure=True)))
        failed = Future()
        failed.set_exception(RuntimeError("tour failed"))
        self.assertFalse(common._pooled_test_was_clean(SimpleNamespace(_result=failed)))
        succeeded = Future()
        succeeded.set_result(True)
        self.assertTrue(common._pooled_test_was_clean(SimpleNamespace(_result=succeeded)))
        cancelled = Future()
        cancelled.cancel()
        self.assertTrue(common._pooled_test_was_clean(SimpleNamespace(_result=cancelled)), "a cancelled result is a normal winddown")

    # Tests [@ANCHOR: zero_sudo:patched_websocket_request]
    def test_browser_close_is_swallowed_for_a_pooled_browser_only(self):
        calls = []
        self.safe_patch_object(common, "original_websocket_request", lambda browser, method, **kw: calls.append(method) or "sent")
        pooled = SimpleNamespace(_hams_chrome=object())
        self.assertIsNone(common._patched_websocket_request(pooled, "Browser.close"))
        self.assertEqual(calls, [], "the class's Chrome must stay up")
        self.assertEqual(common._patched_websocket_request(pooled, "Page.stopLoading"), "sent")
        self.assertEqual(common._patched_websocket_request(SimpleNamespace(), "Browser.close"), "sent", "an unpooled browser still closes")


_EVALUATE_STATE = """JSON.stringify({
    storage: localStorage.getItem('pool_leak'),
    session: sessionStorage.getItem('pool_leak'),
    cookie: document.cookie.indexOf('pool_leak') >= 0,
})"""


@tagged("post_install", "-at_install")
class TestPooledChromeIsolation(HamsHttpCase):
    """Real Chrome. The test methods are numbered: they run in order and the later ones check what the earlier left behind."""

    _pid_of_first_test = None
    _starts_before = None

    def _evaluate(self, browser, expression):
        reply = browser._websocket_request("Runtime.evaluate", params={"expression": expression, "returnByValue": True})
        return reply["result"]["value"]

    def _visit(self, browser):
        with self.allow_requests(browser=browser):
            browser.navigate_to(self.base_url() + "/web/login", wait_stop=True)

    # Tests [@ANCHOR: zero_sudo:pooled_chrome_start]
    # Tests [@ANCHOR: zero_sudo:patched_open_websocket]
    # Tests [@ANCHOR: zero_sudo:patched_read_log]
    def test_01_a_test_leaves_state_in_its_own_context(self):
        browser = ChromeBrowser(self, headless=True)
        try:
            self._visit(browser)
            self._evaluate(browser, "localStorage.setItem('pool_leak', '1'); sessionStorage.setItem('pool_leak', '1'); "
                                    "document.cookie = 'pool_leak=1; path=/'; 'set'")
            self.assertIn('"storage":"1"', self._evaluate(browser, _EVALUATE_STATE), "the page really holds the state")
            self.assertIsInstance(browser.read_log(), bytes)
        finally:
            browser.stop()
        chrome = common._chrome_pool.chrome
        self.assertIsNotNone(chrome, "a clean test hands its Chrome back to the pool")
        self.assertIsNone(chrome.proc.poll(), "and does not stop it")
        type(self)._pid_of_first_test = chrome.proc.pid
        type(self)._starts_before = common._chrome_pool.starts

    # Tests [@ANCHOR: zero_sudo:chrome_pool_acquire]
    # Tests [@ANCHOR: zero_sudo:patched_open_websocket]
    # Tests [@ANCHOR: zero_sudo:headless_chrome_of_this_run]
    # Tests [@ANCHOR: zero_sudo:chrome_pool_new_page]
    # Tests [@ANCHOR: zero_sudo:chrome_pool_end_page]
    # Tests [@ANCHOR: zero_sudo:chrome_pool_dispose_context]
    # Tests [@ANCHOR: zero_sudo:pooled_chrome_alive]
    # Tests [@ANCHOR: zero_sudo:pooled_chrome_pids]
    # Tests [@ANCHOR: zero_sudo:pooled_chrome_log_size]
    # Tests [@ANCHOR: zero_sudo:pooled_chrome_log_since]
    # Tests [@ANCHOR: zero_sudo:pooled_chrome_command]
    # Tests [@ANCHOR: zero_sudo:chrome_pool_error]
    def test_02_the_next_test_gets_the_same_chrome_and_none_of_the_state(self):
        browser = ChromeBrowser(self, headless=True)
        try:
            self.assertEqual(
                common._chrome_pool.chrome.proc.pid,
                self._pid_of_first_test,
                "one Chrome serves the class, and the end-of-test reaper left it alone",
            )
            self.assertEqual(common._chrome_pool.starts, self._starts_before, "no new Chrome was started")
            chrome = common._chrome_pool.chrome
            self.assertTrue(chrome.alive())
            self.assertIn(chrome.proc.pid, chrome.pids())
            self.assertGreaterEqual(chrome.log_since(0), b"")
            self.assertGreaterEqual(chrome.log_size(), 0)
            with self.assertRaises(ChromePoolError):
                chrome.command("No.suchMethod")
            self._visit(browser)
            self.assertEqual(
                self._evaluate(browser, _EVALUATE_STATE),
                '{"storage":null,"session":null,"cookie":false}',
                "local storage, session storage and cookies of the earlier test must not be visible",
            )
        finally:
            browser.stop()

    # Tests [@ANCHOR: zero_sudo:chrome_pool_release]
    # Tests [@ANCHOR: zero_sudo:patched_chrome_stop]
    # Tests [@ANCHOR: zero_sudo:pooled_chrome_stop]
    def test_03_a_crashed_chrome_fails_only_its_test_and_the_next_one_starts_cleanly(self):
        pool = common._chrome_pool
        browser = ChromeBrowser(self, headless=True)
        crashed = pool.chrome
        try:
            try:
                crashed.command("Browser.crash", timeout=3)
            except Exception as e:  # audit-ignore-catch-all: the answer never comes, the connection is the thing that dies
                _logger.info("Browser.crash got no answer, as expected: %s", e)
            deadline = time.monotonic() + 10
            while crashed.proc.poll() is None and time.monotonic() < deadline:
                time.sleep(0.1)  # audit-ignore-sleep: waiting for a real process to die
            self.assertIsNotNone(crashed.proc.poll(), "the Chrome really died")
        finally:
            browser.stop()  # must not raise, and must not leave the dead Chrome in the pool
        self.assertIsNone(pool.chrome, "a Chrome that died during a test is not given to the next one")

        starts = pool.starts
        fresh = ChromeBrowser(self, headless=True)
        try:
            self.assertEqual(pool.starts, starts + 1, "the next test starts a fresh Chrome")
            self.assertIsNot(pool.chrome, crashed)
            self._visit(fresh)
            self.assertEqual(self._evaluate(fresh, "1 + 1"), 2)
        finally:
            fresh.stop()

    # Tests [@ANCHOR: zero_sudo:hams_http_case_teardown_class]
    def test_04_the_chrome_is_stopped_when_the_class_ends(self):
        self.assertIsNotNone(common._chrome_pool.chrome, "still there for the rest of the class")
        self.addClassCleanup(self._assert_pool_empty_after_class)

    @staticmethod
    def _assert_pool_empty_after_class():
        # Class cleanups run after tearDownClass, which closes the pool.
        if common._chrome_pool.chrome is not None:
            raise AssertionError("tearDownClass left the class's Chrome running")
