# -*- coding: utf-8 -*-
# SPDX-License-Identifier: AGPL-3.0-or-later
"""The test harness's own helpers: the transaction-impedance hint, the headless-chrome reaper, the
background-future tracker and the unstarted-patcher variant of safe_patch_object.

These are the pieces of `zero_sudo.tests` that every other module's tests lean on, so a silent
regression in one of them shows up as flaky teardowns elsewhere. Each test drives the real helper;
only the process table (`psutil.process_iter`) and the wrapped `TestCursor.execute` are replaced, so
that nothing real is killed and no real database conflict is needed."""
import concurrent.futures
import os
import threading
from types import SimpleNamespace

import psutil
import psycopg2

from odoo.addons.zero_sudo import tests as zero_sudo_tests
from odoo.addons.zero_sudo.tests import common
from odoo.addons.zero_sudo.tests.common import HamsTransactionCase
from odoo.tests.common import tagged


class _FakeProcess:
    """Just enough of psutil.Process for the reaper: `info`, `kill()` and `wait()`."""

    def __init__(self, name, uid, cmdline):
        self.info = {"pid": 1, "name": name, "uids": SimpleNamespace(real=uid), "cmdline": cmdline}
        self.kills = 0

    def kill(self):
        self.kills += 1

    def wait(self, timeout=None):
        return 0


def _pg_error(pgcode):
    """A psycopg2.Error whose pgcode is `pgcode` (the attribute is read-only on a real instance)."""

    class _PgError(psycopg2.Error):
        pass

    _PgError.pgcode = pgcode
    return _PgError("simulated")


@tagged("post_install", "-at_install")
class TestHarnessHelpers(HamsTransactionCase):
    # Tests [@ANCHOR: zero_sudo:monitored_test_execute]
    def test_a_postgres_conflict_in_the_test_cursor_logs_the_realtransactioncase_hint_and_reraises(self):
        conflict = _pg_error("40001")
        self.safe_patch_object(zero_sudo_tests, "_original_test_cursor_execute", side_effect=conflict)
        with self.assertLogs(zero_sudo_tests._logger, "CRITICAL") as logs:
            with self.assertRaises(psycopg2.Error) as caught:
                zero_sudo_tests._monitored_test_execute(object(), "SELECT 1")
        self.assertIs(caught.exception, conflict)
        self.assertIn("RealTransactionCase", logs.output[0])
        self.assertIn("40001", logs.output[0])

    # Tests [@ANCHOR: zero_sudo:monitored_test_execute]
    def test_other_database_errors_and_normal_results_pass_through_without_the_hint(self):
        unrelated = _pg_error("42P01")
        self.safe_patch_object(zero_sudo_tests, "_original_test_cursor_execute", side_effect=unrelated)
        with self.assertNoLogs(zero_sudo_tests._logger, "CRITICAL"):
            with self.assertRaises(psycopg2.Error):
                zero_sudo_tests._monitored_test_execute(object(), "SELECT 1")
        zero_sudo_tests._original_test_cursor_execute.side_effect = None
        zero_sudo_tests._original_test_cursor_execute.return_value = "rows"
        self.assertEqual(zero_sudo_tests._monitored_test_execute(object(), "SELECT 1", log_exceptions=False), "rows")

    # Tests [@ANCHOR: zero_sudo:kill_all_headless_chrome]
    def test_only_this_uids_headless_chrome_processes_are_killed(self):
        mine = os.getuid()
        victim = _FakeProcess("chromium", mine, ["chromium", "--headless=new"])
        victim_two = _FakeProcess("chrome", mine, ["/opt/chrome", "--remote-debugging-port=0", "--headless"])
        visible_browser = _FakeProcess("chromium", mine, ["chromium"])
        someone_elses = _FakeProcess("chromium", mine + 1, ["chromium", "--headless"])
        not_a_browser = _FakeProcess("python3", mine, ["python3", "--headless"])
        everything = [victim, victim_two, visible_browser, someone_elses, not_a_browser]
        self.safe_patch_object(psutil, "process_iter", lambda *_a, **_k: iter(everything))
        common._kill_all_headless_chrome_for_this_uid()
        self.assertGreaterEqual(victim.kills, 1)
        self.assertGreaterEqual(victim_two.kills, 1)
        self.assertEqual(visible_browser.kills, 0, "a non-headless browser belongs to a person")
        self.assertEqual(someone_elses.kills, 0, "another uid's process must never be touched")
        self.assertEqual(not_a_browser.kills, 0, "only chrome/chromium by name")

    # Tests [@ANCHOR: zero_sudo:tracked_threadpool_submit]
    # Tests [@ANCHOR: zero_sudo:wait_for_background_futures]
    def test_a_submitted_background_job_is_tracked_until_it_finishes_and_a_stall_is_logged(self):
        release = threading.Event()
        executor = concurrent.futures.ThreadPoolExecutor(max_workers=1)
        self.addCleanup(executor.shutdown, wait=True)
        self.addCleanup(release.set)

        future = executor.submit(release.wait, 10)
        self.assertIn(future, common._pending_background_futures)
        self.assertIn("wait", common._pending_background_future_names[future])

        with self.assertLogs(common._logger, "WARNING") as logs:
            common.wait_for_background_futures(timeout=0.2)
        self.assertIn("still running", logs.output[0])
        self.assertIn(future, common._pending_background_futures, "a stalled job stays tracked")

        release.set()
        common.wait_for_background_futures(timeout=5)
        self.assertTrue(future.done())
        self.assertNotIn(future, common._pending_background_futures)
        self.assertNotIn(future, common._pending_background_future_names)

    # Tests [@ANCHOR: zero_sudo:safe_patch_object_cm]
    def test_safe_patch_object_cm_returns_an_unstarted_patcher_that_the_caller_controls(self):
        target = SimpleNamespace(value="original")
        patcher = self.safe_patch_object_cm(target, "value", "patched")
        self.assertEqual(target.value, "original", "nothing is patched until the caller enters it")
        with patcher:
            self.assertEqual(target.value, "patched")
        self.assertEqual(target.value, "original", "the patch ends with the caller's own block, not at teardown")

        defaulted = self.safe_patch_object_cm(target, "value")
        with defaulted as mock_obj:
            self.assertIsInstance(mock_obj, common.DiagnosticMock)

    # Tests [@ANCHOR: zero_sudo:safe_patch_object_cm]
    def test_safe_patch_object_cm_refuses_to_mock_a_database_cursor(self):
        class FakeCursor:
            pass

        with self.assertRaises(RuntimeError):
            self.safe_patch_object_cm(FakeCursor(), "execute")
