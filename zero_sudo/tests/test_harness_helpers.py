# -*- coding: utf-8 -*-
# SPDX-License-Identifier: AGPL-3.0-or-later
"""The test harness's own helpers: the transaction-impedance hint, the headless-chrome reaper, the
background-future tracker and the unstarted-patcher variant of safe_patch_object.

These are the pieces of `zero_sudo.tests` that every other module's tests lean on, so a silent
regression in one of them shows up as flaky teardowns elsewhere. Each test drives the real helper;
only the process table (`psutil.process_iter`) and the wrapped `TestCursor.execute` are replaced, so
that nothing real is killed and no real database conflict is needed."""
import concurrent.futures
import contextlib
import glob
import json
import os
import shutil
import tempfile
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

    def __init__(self, name, uid, cmdline, pid=1):
        self.pid = pid
        self.info = {"pid": pid, "name": name, "uids": SimpleNamespace(real=uid), "cmdline": cmdline}
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
    # Tests [@ANCHOR: zero_sudo:headless_chrome_of_this_run]
    # Tests [@ANCHOR: zero_sudo:profiles_of]
    def test_only_this_runs_headless_chrome_processes_are_killed(self):
        mine = os.getuid()
        with tempfile.TemporaryDirectory(dir=tempfile.gettempdir()) as profile_dir:
            leftover_profile = os.path.join(profile_dir, "left_chrome_odoo")
            os.makedirs(leftover_profile)
            descendant = _FakeProcess("chromium", mine, ["chromium", "--headless=new"], pid=4101)
            leftover = _FakeProcess(
                "chrome", mine, ["/opt/chrome", "--headless", f"--user-data-dir={leftover_profile}"], pid=4102
            )
            another_runs = _FakeProcess(
                "chrome", mine, ["/opt/chrome", "--headless", f"--user-data-dir={profile_dir}/no_such_dir_chrome_odoo"],
                pid=4103,
            )
            no_profile_not_a_child = _FakeProcess("chromium", mine, ["chromium", "--headless"], pid=4104)
            visible_browser = _FakeProcess("chromium", mine, ["chromium"], pid=4105)
            someone_elses = _FakeProcess("chromium", mine + 1, ["chromium", "--headless"], pid=4101)
            not_a_browser = _FakeProcess("python3", mine, ["python3", "--headless"], pid=4101)
            everything = [descendant, leftover, another_runs, no_profile_not_a_child, visible_browser, someone_elses, not_a_browser]
            self.safe_patch_object(psutil, "process_iter", lambda *_a, **_k: iter(everything))
            self.safe_patch_object(
                psutil, "Process", lambda *_a, **_k: SimpleNamespace(children=lambda recursive=False: [SimpleNamespace(pid=4101)])
            )
            common._kill_all_headless_chrome_for_this_uid()
        self.assertGreaterEqual(descendant.kills, 1, "a descendant of this test process is this run's browser")
        self.assertGreaterEqual(leftover.kills, 1, "an orphan whose profile directory is here is this run's leftover")
        self.assertEqual(another_runs.kills, 0, "a browser whose profile directory is not here belongs to another run")
        self.assertEqual(no_profile_not_a_child.kills, 0, "an unrelated headless browser of the same uid belongs to another run")
        self.assertEqual(visible_browser.kills, 0, "a non-headless browser belongs to a person")
        self.assertEqual(someone_elses.kills, 0, "another uid's process must never be touched")
        self.assertEqual(not_a_browser.kills, 0, "only chrome/chromium by name")

    # Tests [@ANCHOR: zero_sudo:kill_all_headless_chrome]
    def test_the_reaper_does_not_wait_when_there_is_no_chrome_to_reap(self):
        """Every HamsHttpCase test's tearDown calls the reaper, nearly always with nothing to kill: it must not sleep then."""
        self.safe_patch_object(common, "_headless_chrome_of_this_run", return_value=[])
        sleeps = self.safe_patch_object(common.time, "sleep")
        common._kill_all_headless_chrome_for_this_uid()
        sleeps.assert_not_called()

    # Tests [@ANCHOR: zero_sudo:kill_all_headless_chrome]
    def test_the_reaper_kills_and_rescans_until_a_scan_finds_nothing(self):
        victim = _FakeProcess("chrome", os.getuid(), ["chrome", "--headless"], pid=4201)
        late_child = _FakeProcess("chrome", os.getuid(), ["chrome", "--headless"], pid=4202)
        self.safe_patch_object(
            common, "_headless_chrome_of_this_run", side_effect=[[victim], [victim, late_child], [late_child], []]
        )
        sleeps = self.safe_patch_object(common.time, "sleep")
        common._kill_all_headless_chrome_for_this_uid()
        self.assertGreaterEqual(victim.kills, 1)
        self.assertGreaterEqual(late_child.kills, 1, "a child that appeared after the first scan is killed too")
        self.assertEqual(sleeps.call_count, 3, "a short poll after each scan that found something, none after the empty one")

    # Tests [@ANCHOR: zero_sudo:kill_all_headless_chrome]
    def test_the_reaper_gives_up_at_its_cap_and_logs_the_survivor(self):
        stubborn = _FakeProcess("chrome", os.getuid(), ["chrome", "--headless"], pid=4301)
        self.safe_patch_object(common, "_headless_chrome_of_this_run", return_value=[stubborn])
        self.safe_patch_object(common.time, "sleep")
        self.safe_patch_object(common, "_CHROME_REAP_CAP_SECONDS", 0.0)
        with self.assertLogs(common._logger, "WARNING") as logs:
            common._kill_all_headless_chrome_for_this_uid()
        self.assertEqual(stubborn.kills, 1, "killed once, then given up on at the (zero) cap")
        self.assertTrue(any("4301" in line and "still present" in line for line in logs.output))

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

    # Tests [@ANCHOR: zero_sudo:invalidate_zero_sudo_utils_cache]
    def test_invalidating_the_utils_cache_does_nothing_when_zero_sudo_is_not_in_the_registry(self):
        """edge_cache's own run has no `zero_sudo.security.utils` model, and must not reach Redis for it."""
        without_model = SimpleNamespace(registry={}, env=self.env)
        invalidate = self.safe_patch_object(common, "invalidate_model_cache")
        common.invalidate_zero_sudo_utils_cache(without_model)
        invalidate.assert_not_called()

    # Tests [@ANCHOR: zero_sudo:invalidate_zero_sudo_utils_cache]
    def test_invalidating_the_utils_cache_runs_when_the_model_is_installed(self):
        with_model = SimpleNamespace(registry={"zero_sudo.security.utils": object()}, env=self.env)
        invalidate = self.safe_patch_object(common, "invalidate_model_cache")
        common.invalidate_zero_sudo_utils_cache(with_model)
        invalidate.assert_called_once_with(self.env, "zero_sudo.security.utils")

    # Tests [@ANCHOR: zero_sudo:url_is_at_origin]
    def test_an_origin_matches_only_at_a_real_authority_boundary(self):
        origin = "http://origin.test:8069"
        self.assertTrue(common._url_is_at_origin(origin, origin))
        self.assertTrue(common._url_is_at_origin(origin + "/web", origin))
        self.assertTrue(common._url_is_at_origin(origin + "?a=1", origin))
        self.assertTrue(common._url_is_at_origin(origin + "#top", origin))
        self.assertFalse(common._url_is_at_origin("http://origin.test:80699/x", origin), "another port is another service")
        self.assertFalse(common._url_is_at_origin("http://origin.test:8069@example.com/", origin), "userinfo is not the host")
        self.assertFalse(common._url_is_at_origin("http://origin.test:8069:9/", origin))
        self.assertFalse(common._url_is_at_origin("https://elsewhere.example/", origin))
        self.assertTrue(common._url_is_at_origin("http://host:99/x", "http://host", allow_any_port=True))
        self.assertFalse(common._url_is_at_origin("http://host:99/x", "http://host"))

    # Tests [@ANCHOR: zero_sudo:protected_pids]
    def test_the_protected_pids_are_this_process_init_and_every_ancestor(self):
        protected = common._protected_pids()
        self.assertTrue({0, 1, os.getpid(), os.getppid()} <= protected)

    def _fake_browser_tree(self, browser_pid, children):
        """psutil.Process as the clean-up sees it: `children` below `browser_pid`, no ancestors."""

        def fake_process(pid=None):
            below = children if pid == browser_pid else []
            return SimpleNamespace(parents=lambda: [], children=lambda recursive=False: below)

        self.safe_patch_object(psutil, "Process", fake_process)

    # Tests [@ANCHOR: zero_sudo:kill_browser_tree]
    # Tests [@ANCHOR: zero_sudo:protected_pids]
    def test_a_live_browser_tree_is_killed_but_never_this_process_or_a_foreign_group(self):
        browser = SimpleNamespace(pid=900001, poll=lambda: None, returncode=None)
        below = _FakeProcess("chrome", os.getuid(), [], pid=900002)
        ourselves = _FakeProcess("python3", os.getuid(), [], pid=os.getpid())
        self._fake_browser_tree(browser.pid, [below, ourselves])
        getpgid = self.safe_patch_object(common.os, "getpgid", return_value=900001)
        group_signal = self.safe_patch_object(common.os, "killpg")
        common._kill_browser_tree(browser, "test")
        self.assertEqual(below.kills, 1, "a descendant of the browser is killed")
        self.assertEqual(ourselves.kills, 0, "this process is never signalled, even when found below the browser")
        group_signal.assert_called_once_with(900001, common.signal.SIGKILL)
        getpgid.assert_called_with(900001)

    # Tests [@ANCHOR: zero_sudo:kill_browser_tree]
    def test_the_browser_clean_up_leaves_our_own_process_group_and_exited_browsers_alone(self):
        self._fake_browser_tree(900003, [])
        group_signal = self.safe_patch_object(common.os, "killpg")
        self.safe_patch_object(common.os, "getpgid", return_value=os.getpgrp())
        common._kill_browser_tree(SimpleNamespace(pid=900003, poll=lambda: None, returncode=None), "test")
        group_signal.assert_not_called()
        exited = SimpleNamespace(pid=900004, poll=lambda: 0, returncode=0)
        common._kill_browser_tree(exited, "test")
        common._kill_browser_tree(SimpleNamespace(pid=1, poll=lambda: None, returncode=None), "test")
        group_signal.assert_not_called()

    def _coverage_browser(self, responses):
        calls = []

        def request(method, params=None):
            calls.append(method)
            if isinstance(responses.get(method), Exception):
                raise responses[method]
            return responses.get(method)

        browser = SimpleNamespace(
            _websocket_request=request,
            cleanup=contextlib.ExitStack(),
            test_case=SimpleNamespace(id=lambda: "zero_sudo.fake_coverage_test"),
        )
        return browser, calls

    def _coverage_dir(self):
        out_dir = tempfile.mkdtemp(prefix="hams_js_coverage_test_")
        self.addCleanup(shutil.rmtree, out_dir, ignore_errors=True)
        os.environ["HAMS_JS_COVERAGE_DIR"] = out_dir
        self.addCleanup(os.environ.pop, "HAMS_JS_COVERAGE_DIR", None)
        return out_dir

    # Tests [@ANCHOR: zero_sudo:js_coverage_start]
    def test_js_coverage_starts_only_when_asked_and_never_fails_the_test(self):
        browser, calls = self._coverage_browser({})
        os.environ.pop("HAMS_JS_COVERAGE_DIR", None)
        common._js_coverage_start(browser)
        self.assertEqual(calls, [], "unset, nothing runs")
        self._coverage_dir()
        common._js_coverage_start(browser)
        self.assertEqual(calls, ["Profiler.enable", "Debugger.enable", "Profiler.startPreciseCoverage"])
        self.assertTrue(browser._hams_js_coverage_on)
        failing, _calls = self._coverage_browser({"Profiler.enable": TimeoutError("no answer")})
        common._js_coverage_start(failing)
        self.assertNotIn("_hams_js_coverage_on", vars(failing))

    # Tests [@ANCHOR: zero_sudo:js_coverage_write]
    # Tests [@ANCHOR: zero_sudo:js_coverage_start]
    def test_js_coverage_is_written_when_the_browser_shuts_down(self):
        out_dir = self._coverage_dir()
        browser, calls = self._coverage_browser({
            "Profiler.takePreciseCoverage": {"result": [
                {"url": "http://x.test/web/assets/1/web.min.js", "scriptId": "7", "functions": [{"functionName": "f"}]},
                {"url": "http://x.test/not_a_bundle.js", "scriptId": "8", "functions": []},
            ]},
            "Debugger.getScriptSource": {"scriptSource": "var a;"},
        })
        common._js_coverage_start(browser)
        browser.cleanup.close()  # runs _js_coverage_write, as browser_js does
        self.assertFalse(browser._hams_js_coverage_on)
        records = glob.glob(os.path.join(out_dir, "*.json"))
        self.assertEqual(len(records), 1)
        with open(records[0], encoding="utf-8") as f:
            record = json.load(f)
        self.assertEqual(record["test"], "zero_sudo.fake_coverage_test")
        self.assertEqual([s["url"] for s in record["scripts"]], ["http://x.test/web/assets/1/web.min.js"])
        self.assertEqual(record["bundles"], {"http://x.test/web/assets/1/web.min.js": "var a;"})
        before = list(calls)
        common._js_coverage_write(browser)
        self.assertEqual(calls, before, "a browser whose coverage is already written is left alone")
