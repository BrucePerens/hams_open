# -*- coding: utf-8 -*-
# SPDX-License-Identifier: AGPL-3.0-or-later
"""A Werkzeug thread that cannot be interrupted is reported once and not waited on again.

An injected SystemExit is only raised when the thread next runs Python bytecode, so a thread blocked in a C call (here a lock
acquire, standing in for a socket read) never sees it. Every later test's teardown used to re-wait the full timeout on it."""
import threading
import time

from odoo.addons.zero_sudo.tests import common
from odoo.addons.zero_sudo.tests.common import HamsTransactionCase
from odoo.tests.common import tagged


@tagged("post_install", "-at_install")
class TestWerkzeugThreadWait(HamsTransactionCase):
    # Tests [@ANCHOR: zero_sudo:wait_for_werkzeug_threads]
    def test_an_uninterruptible_thread_is_waited_on_once_then_skipped(self):
        release = threading.Event()
        blocked = threading.Lock()
        blocked.acquire()

        def stuck():
            blocked.acquire()  # a C-level wait: the injected SystemExit cannot reach it
            release.set()

        thread = threading.Thread(target=stuck, name="stuck-werkzeug-test", daemon=True)  # burn-ignore-test-daemon-thread: a deliberately stuck stand-in, released and joined in the cleanup below
        thread.start()
        common._active_werkzeug_threads.add(thread)

        def cleanup():
            blocked.release()
            thread.join(2)
            common._active_werkzeug_threads.discard(thread)
            common._abandoned_werkzeug_threads.discard(thread)

        self.addCleanup(cleanup)

        first = time.time()
        common.wait_for_werkzeug_threads(timeout=0.3)
        self.assertGreaterEqual(time.time() - first, 0.3, "the first teardown does wait for the thread")
        self.assertIn(thread, common._abandoned_werkzeug_threads)
        self.assertIn(thread, common._active_werkzeug_threads, "it stays tracked until it really returns")

        second = time.time()
        common.wait_for_werkzeug_threads(timeout=3.0)
        self.assertLess(time.time() - second, 1.0, "a thread already proven un-interruptible is not waited on again")
