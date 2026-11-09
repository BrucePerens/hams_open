# -*- coding: utf-8 -*-
# SPDX-License-Identifier: AGPL-3.0-or-later
"""One headless Chrome process shared by the browser tests of one test class.

Odoo's `ChromeBrowser` starts a Chrome for every browser test and stops it again: about 0.7 s to start and 0.5 to 1 s to stop,
the larger part of a 2.5 s tour. `ChromePool` keeps one Chrome alive while the tests of one class run and gives each test
its own **browser context** (CDP `Target.createBrowserContext`, an incognito-style profile): cookies, local and session
storage, IndexedDB, the Cache API, service workers and the HTTP cache of one test are invisible to the next, and
`Target.disposeBrowserContext` at the end of the test destroys all of them and the test's renderers.

What is shared is the browser process, the GPU process and the profile directory, never a page's state.

A Chrome is reused only after a clean test. After a failed or timed-out test, an unclean teardown, a dead process or an
unresponsive DevTools endpoint the Chrome is killed and the next test starts a fresh one, so a crashed Chrome fails exactly
the test that was using it. The pool also restarts the Chrome when the test class changes, when a test wants different launch
switches (touch events) and after `MAX_USES` leases, which bounds the memory a long class can accumulate.

The process and the profile directory are created by a `start` callable the harness supplies (it runs Odoo's own
`_chrome_start`, with the harness's process-group, namespace and reaper protections), so this module has no import of it."""

import json
import logging
import os
import shutil
import threading
import time

import psutil
import requests
from odoo.tests.common import HOST, websocket

_logger = logging.getLogger(__name__)

# Leases (tests) one Chrome serves before it is replaced: a long class cannot grow a Chrome without bound.
MAX_USES = 40


# [@ANCHOR: zero_sudo:chrome_pool_error]
class ChromePoolError(Exception):
    """A DevTools command to the pooled Chrome failed: the Chrome is not usable and is replaced."""


# [@ANCHOR: zero_sudo:pooled_chrome]
class PooledChrome:
    """One live Chrome of the pool: its Popen object, DevTools port, profile directory and who it was started for."""

    # [@ANCHOR: zero_sudo:pooled_chrome_init]
    def __init__(self, proc, devtools_port, user_data_dir, stop, owner, touch_enabled):
        self.proc = proc
        self.devtools_port = devtools_port
        self.user_data_dir = user_data_dir
        self.owner = owner
        self.touch_enabled = touch_enabled
        self.uses = 0
        self._stop = stop

    # [@ANCHOR: zero_sudo:pooled_chrome_alive]
    def alive(self):
        """True while the process runs and its DevTools endpoint answers."""
        if self.proc.poll() is not None:
            return False
        try:
            return requests.get(f"http://{HOST}:{self.devtools_port}/json/version", timeout=3).ok
        except requests.RequestException:
            return False

    # [@ANCHOR: zero_sudo:pooled_chrome_pids]
    def pids(self):
        """The pids of the Chrome and of everything below it (renderers, GPU process), whichever PID namespace they run in."""
        pids = {self.proc.pid}
        try:
            pids.update(child.pid for child in psutil.Process(self.proc.pid).children(recursive=True))
        except psutil.Error as e:
            _logger.debug("Could not list the pooled Chrome's descendants: %s", e)
        return pids

    # [@ANCHOR: zero_sudo:pooled_chrome_log_size]
    def log_size(self):
        try:
            return os.path.getsize(os.path.join(self.user_data_dir, "chrome_debug.log"))
        except OSError:
            return 0

    # [@ANCHOR: zero_sudo:pooled_chrome_log_since]
    def log_since(self, offset):
        """Bytes the Chrome has logged since `offset` (a `log_size()`): the part of its log that belongs to one test."""
        try:
            with open(os.path.join(self.user_data_dir, "chrome_debug.log"), "rb") as log:
                log.seek(offset)
                return log.read()
        except OSError:
            return b""

    # [@ANCHOR: zero_sudo:pooled_chrome_command]
    def command(self, method, params=None, timeout=10.0):
        """Runs one DevTools command on the browser itself (not on a page) over a short-lived websocket and returns its result."""
        version = requests.get(f"http://{HOST}:{self.devtools_port}/json/version", timeout=3)
        version.raise_for_status()
        ws = websocket.create_connection(
            version.json()["webSocketDebuggerUrl"], timeout=timeout, suppress_origin=True
        )
        try:
            ws.send(json.dumps({"id": 1, "method": method, "params": params or {}}))
            deadline = time.monotonic() + timeout
            while time.monotonic() < deadline:
                reply = json.loads(ws.recv())
                if reply.get("id") == 1:
                    if "error" in reply:
                        raise ChromePoolError(f"{method}: {reply['error'].get('message')}")
                    return reply.get("result", {})
            raise ChromePoolError(f"{method}: no answer within {timeout} s")
        finally:
            ws.close()

    # [@ANCHOR: zero_sudo:pooled_chrome_stop]
    def stop(self):
        """Terminates the Chrome and removes its profile directory. Safe to call twice."""
        stop, self._stop = self._stop, None
        if stop is not None:
            try:
                stop()
            except Exception as e:  # audit-ignore-catch-all
                _logger.warning("Stopping the pooled Chrome raised: %s", repr(e))
        shutil.rmtree(self.user_data_dir, ignore_errors=True)


# [@ANCHOR: zero_sudo:chrome_pool]
class ChromePool:
    """At most one live Chrome, leased to one browser test at a time (Odoo runs tests of a process one after the other).

    `start(touch_enabled)` returns `(proc, devtools_port, user_data_dir, stop)` for a new headless Chrome."""

    # [@ANCHOR: zero_sudo:chrome_pool_init]
    def __init__(self, start, max_uses=MAX_USES):
        self._start = start
        self._max_uses = max_uses
        self._lock = threading.RLock()
        self._chrome = None
        self.starts = 0  # Chromes started so far: what the tests and the log use to show how much was reused

    @property
    # [@ANCHOR: zero_sudo:chrome_pool_chrome]
    def chrome(self):
        return self._chrome

    # [@ANCHOR: zero_sudo:chrome_pool_acquire]
    def acquire(self, owner, touch_enabled):
        """The live Chrome for a test of class `owner`, started if there is none usable. A Chrome started for another class,
        with other switches, past `MAX_USES`, dead or not answering is stopped first."""
        with self._lock:
            chrome = self._chrome
            if chrome is not None:
                reason = None
                if chrome.owner is not owner:
                    reason = "another test class"
                elif chrome.touch_enabled != touch_enabled:
                    reason = "other launch switches"
                elif chrome.uses >= self._max_uses:
                    reason = "it has served %d tests" % chrome.uses
                elif not chrome.alive():
                    reason = "it is not running or not answering"
                if reason is not None:
                    _logger.info("Replacing the pooled Chrome: %s", reason)
                    self.close()
            if self._chrome is None:
                proc, port, user_data_dir, stop = self._start(touch_enabled)
                self._chrome = PooledChrome(proc, port, user_data_dir, stop, owner, touch_enabled)
                self.starts += 1
            self._chrome.uses += 1
            return self._chrome

    # [@ANCHOR: zero_sudo:chrome_pool_new_page]
    def new_page(self, chrome):
        """A fresh browser context with one blank page in it; returns `(context_id, websocket_url_of_the_page)`."""
        context_id = chrome.command("Target.createBrowserContext", {"disposeOnDetach": False})["browserContextId"]
        try:
            target_id = chrome.command("Target.createTarget", {"url": "about:blank", "browserContextId": context_id})["targetId"]
        except ChromePoolError:
            self._dispose_context(chrome, context_id)
            raise
        return context_id, f"ws://{HOST}:{chrome.devtools_port}/devtools/page/{target_id}"

    @staticmethod
    # [@ANCHOR: zero_sudo:chrome_pool_dispose_context]
    def _dispose_context(chrome, context_id):
        try:
            chrome.command("Target.disposeBrowserContext", {"browserContextId": context_id})
            return True
        except Exception as e:  # audit-ignore-catch-all: a crashed Chrome raises whatever its socket does; the caller marks the Chrome dirty
            _logger.warning("Could not dispose the test's browser context: %s", repr(e))
            return False

    # [@ANCHOR: zero_sudo:chrome_pool_end_page]
    def end_page(self, chrome, context_id):
        """Destroys a test's browser context (its pages, storage, cookies, service workers). False when that failed, which
        makes the Chrome untrustworthy: the caller releases it as dirty."""
        return self._dispose_context(chrome, context_id)

    # [@ANCHOR: zero_sudo:chrome_pool_release]
    def release(self, chrome, clean):
        """The test is over. A Chrome after an unclean test is stopped, so the next test gets a fresh one."""
        with self._lock:
            if not clean and self._chrome is chrome:
                _logger.info("Stopping the pooled Chrome after a test that did not end cleanly")
                self.close()

    # [@ANCHOR: zero_sudo:chrome_pool_close]
    def close(self):
        """Stops the live Chrome, if any."""
        with self._lock:
            chrome, self._chrome = self._chrome, None
        if chrome is not None:
            chrome.stop()

    # [@ANCHOR: zero_sudo:chrome_pool_owned_pids]
    def owned_pids(self):
        """Pids of the live Chrome and its descendants: what the harness's reapers must never kill while it is leased."""
        with self._lock:
            chrome = self._chrome
        return chrome.pids() if chrome is not None else set()

    # [@ANCHOR: zero_sudo:chrome_pool_owns_profile]
    def owns_profile(self, profile_dir):
        with self._lock:
            chrome = self._chrome
        return chrome is not None and os.path.realpath(chrome.user_data_dir) == os.path.realpath(profile_dir)
