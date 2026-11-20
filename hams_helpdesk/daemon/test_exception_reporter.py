# Copyright © Bruce Perens K6BP.
# SPDX-License-Identifier: AGPL-3.0-or-later
"""Tests [@ANCHOR: hams_helpdesk:daemon_report_unit] and [@ANCHOR: hams_helpdesk:daemon_post_report] without Odoo: the
OnFailure unit waits out an Odoo restart instead of exiting 1 and leaving a failed hams-exception-report@ instance.
    python3 hams_helpdesk/daemon/test_exception_reporter.py
"""
import io
import json
import os
import shutil
import socketserver
import sys
import tempfile
import threading
import time
import unittest
import urllib.error
from http.server import BaseHTTPRequestHandler, HTTPServer
from contextlib import redirect_stderr
from unittest import mock

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import exception_reporter as er  # noqa: E402


class Clock:
    def __init__(self):
        self.now = 0.0
        self.sleeps = []

    def monotonic(self):
        return self.now

    def sleep(self, seconds):
        self.sleeps.append(seconds)
        self.now += seconds


def refused():
    return er.OdooUnavailable("could not reach Odoo (URLError)")


class ReportUnitWaitTests(unittest.TestCase):
    def run_report(self, side_effects, wait=600):
        clock = Clock()
        with mock.patch.object(er, "unit_result", return_value="exit-code"), \
                mock.patch.object(er, "journal_tail", return_value="tail"), \
                mock.patch.object(er, "post_report", side_effect=side_effects) as post, \
                redirect_stderr(io.StringIO()):
            try:
                answer = er.report_unit("club.crawl.service", wait_seconds=wait, sleep=clock.sleep, monotonic=clock.monotonic)
            except er.ReportError as error:
                answer = error
        return answer, post, clock

    def test_delivered_first_time(self):
        answer, post, clock = self.run_report([{"recorded": True}])
        self.assertEqual(answer, {"recorded": True})
        self.assertEqual(clock.sleeps, [])
        self.assertEqual(post.call_args[0][0]["unit"], "club.crawl.service")

    def test_odoo_down_for_a_while_is_waited_out_with_backoff(self):
        answer, post, clock = self.run_report([refused(), refused(), refused(), {"recorded": True}])
        self.assertEqual(answer, {"recorded": True})
        self.assertEqual(clock.sleeps, [5, 10, 20])
        # The journal and result are read once, not on every try.
        self.assertEqual({id(call[0][0]) for call in post.call_args_list}.__len__(), 1)

    # Tests [@ANCHOR: hams_helpdesk_exception_reporter_odoo_unavailable]
    def test_gives_up_after_the_budget_with_the_last_error(self):
        answer, post, clock = self.run_report([refused()] * 100, wait=600)
        self.assertIsInstance(answer, er.OdooUnavailable)
        self.assertLessEqual(clock.now, 600)
        self.assertGreater(clock.now, 400)
        self.assertEqual(clock.sleeps[:4], [5, 10, 20, 40])
        self.assertLessEqual(max(clock.sleeps), er.REPORT_BACKOFF_MAX_SECONDS)

    # Tests [@ANCHOR: hams_helpdesk_exception_reporter_report_error]
    def test_a_rejected_key_is_not_retried(self):
        answer, post, clock = self.run_report([er.ReportError("Odoo answered HTTP 403")])
        self.assertIsInstance(answer, er.ReportError)
        self.assertNotIsInstance(answer, er.OdooUnavailable)
        self.assertEqual(post.call_count, 1)
        self.assertEqual(clock.sleeps, [])

    # Tests [@ANCHOR: hams_helpdesk_browser_exception_api_browser_exception_api]
    def test_the_unit_timeout_outlasts_the_wait(self):
        here = os.path.dirname(os.path.abspath(__file__))
        with open(os.path.join(here, "hams-exception-report@.service"), encoding="utf-8") as handle:
            text = handle.read()
        line = [part for part in text.splitlines() if part.startswith("TimeoutStartSec=")][0]
        self.assertGreaterEqual(int(line.split("=")[1]), er.REPORT_ODOO_WAIT_SECONDS + 120)


class MaintenanceTests(unittest.TestCase):
    # Tests [@ANCHOR: hams_helpdesk:daemon_in_maintenance]
    """A unit that fails while the pagerduty maintenance flag is set (a deploy or restart) is not reported (ticket 168)."""

    def run_report(self, flag_json):
        with tempfile.TemporaryDirectory() as tmp:
            path = os.path.join(tmp, "maintenance")
            if flag_json is not None:
                with open(path, "w", encoding="utf-8") as handle:
                    handle.write(flag_json)
            with mock.patch.dict(os.environ, {"PAGERDUTY_MAINTENANCE_FILE": path}), \
                    mock.patch.object(er, "unit_result", return_value="exit-code"), \
                    mock.patch.object(er, "journal_tail", return_value="tail"), \
                    mock.patch.object(er, "post_report", return_value={"recorded": True}) as post, \
                    redirect_stderr(io.StringIO()):
                answer = er.report_unit("adif.processor.service")
        return answer, post

    @staticmethod
    def flag(set_ago, until_in):
        now = time.time()
        return json.dumps({"set_at": now - set_ago, "until": now + until_in, "reason": "deploy", "set_by": "t"})

    def test_an_active_flag_skips_the_report(self):
        answer, post = self.run_report(self.flag(60, 600))
        post.assert_not_called()
        self.assertEqual(answer, {"skipped": "pagerduty maintenance"})

    def test_no_flag_reports(self):
        answer, post = self.run_report(None)
        post.assert_called_once()
        self.assertEqual(answer, {"recorded": True})

    def test_an_expired_flag_reports(self):
        _answer, post = self.run_report(self.flag(1200, -600))
        post.assert_called_once()

    def test_a_malformed_flag_reports(self):
        _answer, post = self.run_report("not json")
        post.assert_called_once()


class PostReportErrorClassTests(unittest.TestCase):
    def post(self, error):
        with mock.patch.object(er, "load_key", return_value="k"), mock.patch.object(er._OPENER, "open", side_effect=error):
            with self.assertRaises(er.ReportError) as caught:
                er.post_report({"unit": "a.service"}, url="http://odoo:8069")
        return caught.exception

    def test_connection_refused_is_unavailable(self):
        self.assertIsInstance(self.post(urllib.error.URLError(ConnectionRefusedError())), er.OdooUnavailable)

    def test_a_timeout_is_unavailable(self):
        self.assertIsInstance(self.post(TimeoutError()), er.OdooUnavailable)

    def test_a_proxy_503_is_unavailable(self):
        error = urllib.error.HTTPError("http://odoo", 503, "x", {}, None)
        self.assertIsInstance(self.post(error), er.OdooUnavailable)

    def test_a_403_is_not(self):
        error = urllib.error.HTTPError("http://odoo", 403, "x", {}, None)
        self.assertNotIsInstance(self.post(error), er.OdooUnavailable)

    def test_the_message_never_holds_the_url(self):
        self.assertNotIn("odoo:8069", str(self.post(urllib.error.URLError(ConnectionRefusedError()))))


class MainTests(unittest.TestCase):
    # Tests [@ANCHOR: hams_helpdesk:daemon_main]
    def run_main(self, argv, **patches):
        err = io.StringIO()
        out = io.StringIO()
        with mock.patch.object(er, "report_unit", **patches) as report, redirect_stderr(err), \
                mock.patch("sys.stdout", out):
            code = er.main(argv)
        return code, report, out.getvalue(), err.getvalue()

    def test_report_unit_reports_the_named_unit_and_exits_zero(self):
        code, report, out, _err = self.run_main(["report-unit", "club.crawl.service"], return_value={"recorded": True})
        self.assertEqual(code, 0)
        report.assert_called_once_with("club.crawl.service")
        self.assertIn('{"recorded": true}', out)

    def test_a_delivery_failure_exits_one_and_says_only_the_error(self):
        code, _report, _out, err = self.run_main(["report-unit", "a.service"], side_effect=er.ReportError("Odoo answered HTTP 403"))
        self.assertEqual(code, 1)
        self.assertIn("Odoo answered HTTP 403", err)

    def test_wrong_arguments_print_usage_and_exit_two(self):
        for argv in ([], ["report-unit"], ["other", "a.service"], ["report-unit", "a", "b"]):
            code, report, _out, err = self.run_main(argv)
            self.assertEqual(code, 2, argv)
            self.assertIn("usage:", err)
            report.assert_not_called()


class ReportingFailuresTests(unittest.TestCase):
    # Tests [@ANCHOR: hams_helpdesk:daemon_reporting_failures_init]
    # Tests [@ANCHOR: hams_helpdesk:daemon_reporting_failures_enter]
    # Tests [@ANCHOR: hams_helpdesk:daemon_reporting_failures_exit]
    def test_an_exception_is_reported_once_and_propagates_unchanged(self):
        guard = er.reporting_failures("club.crawl.service", key_file="/nope")
        self.assertEqual((guard.daemon_name, guard.options), ("club.crawl.service", {"key_file": "/nope"}))
        with mock.patch.object(er, "report_exception") as report:
            with self.assertRaises(KeyError):
                with guard as entered:
                    self.assertIs(entered, guard)
                    raise KeyError("x")
        report.assert_called_once()
        self.assertEqual(report.call_args[0][0], "club.crawl.service")
        self.assertEqual(report.call_args[1], {"key_file": "/nope"})

    def test_a_stop_signal_and_a_clean_exit_report_nothing(self):
        with mock.patch.object(er, "report_exception") as report:
            with er.reporting_failures("a.service"):
                pass
            with self.assertRaises(SystemExit):
                with er.reporting_failures("a.service"):
                    raise SystemExit(0)
        report.assert_not_called()


if __name__ == "__main__":
    unittest.main()


class _RedirectProbe:
    """Two real local servers: `url` answers every request with a 302 to the second, `sink`, which records the headers of
    whatever reaches it. A credential header that follows the redirect shows up in `sink_headers`; `first_headers` holds
    what the first server received, which proves the request did carry the credential."""

    def __init__(self):
        outer = self
        self.first_headers: list[dict[str, str]] = []
        self.sink_headers: list[dict[str, str]] = []

        class Sink(BaseHTTPRequestHandler):
            def _answer(self):
                outer.sink_headers.append({k.lower(): v for k, v in self.headers.items()})
                body = b"{}"
                self.send_response(200)
                self.send_header("Content-Length", str(len(body)))
                self.end_headers()
                self.wfile.write(body)

            do_GET = do_POST = do_PUT = do_PATCH = _answer

            def log_message(self, *args):
                pass

        self.sink = HTTPServer(("127.0.0.1", 0), Sink)  # burn-ignore-test-local-server: hermetic stand-in for the host a redirect points at
        sink_url = f"http://127.0.0.1:{self.sink.server_port}/elsewhere"  # burn-ignore-test-local-server: hermetic stand-in for the host a redirect points at

        class First(BaseHTTPRequestHandler):
            def _answer(self):
                length = int(self.headers.get("Content-Length") or 0)
                if length:
                    self.rfile.read(length)
                outer.first_headers.append({k.lower(): v for k, v in self.headers.items()})
                self.send_response(302)
                self.send_header("Location", sink_url)
                self.send_header("Content-Length", "0")
                self.end_headers()

            do_GET = do_POST = do_PUT = do_PATCH = _answer

            def log_message(self, *args):
                pass

        self.first = HTTPServer(("127.0.0.1", 0), First)  # burn-ignore-test-local-server: hermetic stand-in for the credentialed API that answers with a redirect
        self.url = f"http://127.0.0.1:{self.first.server_port}"  # burn-ignore-test-local-server: hermetic stand-in for the credentialed API
        self._threads = [threading.Thread(target=s.serve_forever, daemon=True) for s in (self.sink, self.first)]  # burn-ignore-test-daemon-thread: two daemon threads of a hermetic fixture, both shut down in __exit__

    def __enter__(self):
        for thread in self._threads:
            thread.start()
        return self

    def __exit__(self, *exc):
        for server in (self.first, self.sink):
            server.shutdown()
            server.server_close()


class UnixSocketTests(unittest.TestCase):
    # Tests [@ANCHOR: hams_helpdesk:daemon_post_report]
    # [@ANCHOR: hams_helpdesk:test_a_unix_url_reaches_odoo_over_the_socket]
    def test_a_unix_url_reaches_odoo_over_the_socket(self):
        # Tests [@ANCHOR: hams_helpdesk:daemon_unix_opener]
        seen = []

        class Guard(BaseHTTPRequestHandler):
            def do_POST(self):
                seen.append((self.path, self.headers["Host"], self.headers["Authorization"], self.rfile.read(int(self.headers["Content-Length"]))))
                body = b'{"recorded": true}'
                self.send_response(200)
                self.send_header("Content-Length", str(len(body)))
                self.end_headers()
                self.wfile.write(body)

            def log_message(self, *args):
                pass

        with tempfile.TemporaryDirectory() as directory:
            key_file = os.path.join(directory, "key")
            with open(key_file, "w", encoding="utf-8") as handle:
                handle.write("ODOO_RPC_KEY=secret-key\n")
            socket_path = os.path.join(directory, "rpc.sock")
            server = socketserver.UnixStreamServer(socket_path, Guard)
            thread = threading.Thread(target=server.serve_forever, daemon=True)  # burn-ignore-test-daemon-thread: hermetic fixture, shut down below
            thread.start()
            try:
                answer = er.post_report({"unit": "a.service"}, url="unix://" + socket_path, key_file=key_file)
            finally:
                server.shutdown()
                server.server_close()
        self.assertEqual(answer, {"recorded": True})
        self.assertEqual(seen, [("/json/2/hams_helpdesk.exception.fingerprint/report_daemon_failure", "unix-socket", "Bearer secret-key", b'{"unit": "a.service"}')])

    def test_a_unix_url_with_nothing_listening_is_unavailable(self):
        with tempfile.TemporaryDirectory() as directory:
            key_file = os.path.join(directory, "key")
            with open(key_file, "w", encoding="utf-8") as handle:
                handle.write("ODOO_RPC_KEY=secret-key\n")
            with self.assertRaises(er.OdooUnavailable):
                er.post_report({}, url="unix://" + os.path.join(directory, "none.sock"), key_file=key_file)


class RefusalTests(unittest.TestCase):
    def test_a_key_file_without_the_key_is_refused(self):
        # Tests [@ANCHOR: hams_helpdesk:daemon_load_key]
        path = os.path.join(tempfile.mkdtemp(), "key.env")
        self.addCleanup(shutil.rmtree, os.path.dirname(path), ignore_errors=True)
        with open(path, "w", encoding="utf-8") as handle:
            handle.write("# comment\nOTHER=1\nODOO_RPC_KEY=\n")
        with self.assertRaises(er.ReportError) as raised:
            er.load_key(path)
        self.assertIn("no ODOO_RPC_KEY", str(raised.exception))

    def test_an_answer_that_is_not_json_is_a_report_error_without_the_body(self):
        # Tests [@ANCHOR: hams_helpdesk:daemon_post_report]
        response = mock.MagicMock()
        response.__enter__.return_value.read.return_value = b"<html>secret-body</html>"
        with mock.patch.object(er, "load_key", return_value="k"), mock.patch.object(er._OPENER, "open", return_value=response):
            with self.assertRaises(er.ReportError) as raised:
                er.post_report({"unit": "a.service"}, url="http://odoo:8069")
        self.assertNotIsInstance(raised.exception, er.OdooUnavailable)
        self.assertNotIn("secret-body", str(raised.exception))

    def test_an_unreadable_journal_or_unit_result_is_empty_text(self):
        # Tests [@ANCHOR: hams_helpdesk:daemon_journal_tail]
        # Tests [@ANCHOR: hams_helpdesk:daemon_unit_result]
        with mock.patch.object(er.subprocess, "run", side_effect=FileNotFoundError("journalctl")):
            self.assertEqual(er.journal_tail("a.service"), "")
            self.assertEqual(er.unit_result("a.service"), "")


class PostReportRefusesARedirectTests(unittest.TestCase):
    def test_a_redirect_from_odoo_is_refused_and_the_key_never_reaches_the_second_host(self):
        # Tests [@ANCHOR: hams_helpdesk:daemon_no_redirect]
        # Tests [@ANCHOR: hams_helpdesk:hams_helpdesk_daemon_exception_reporter_no_redirect]
        # Tests [@ANCHOR: hams_helpdesk:daemon_post_report]
        with _RedirectProbe() as probe, mock.patch.object(er, "load_key", return_value="reporter-key"):
            with self.assertRaises(er.ReportError) as raised:
                er.post_report({"unit": "a.service"}, url=probe.url, database="db")
        self.assertIn("302", str(raised.exception))
        self.assertEqual(probe.first_headers[0]["authorization"], "Bearer reporter-key")
        self.assertEqual(probe.sink_headers, [], "the redirect target must never be asked")
