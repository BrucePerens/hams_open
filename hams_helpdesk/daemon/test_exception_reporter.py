# Copyright © Bruce Perens K6BP.
# SPDX-License-Identifier: AGPL-3.0-or-later
"""Tests [@ANCHOR: hams_helpdesk:daemon_report_unit] and [@ANCHOR: hams_helpdesk:daemon_post_report] without Odoo: the
OnFailure unit waits out an Odoo restart instead of exiting 1 and leaving a failed hams-exception-report@ instance.
    python3 hams_helpdesk/daemon/test_exception_reporter.py
"""
import io
import os
import sys
import threading
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

    def test_gives_up_after_the_budget_with_the_last_error(self):
        answer, post, clock = self.run_report([refused()] * 100, wait=600)
        self.assertIsInstance(answer, er.OdooUnavailable)
        self.assertLessEqual(clock.now, 600)
        self.assertGreater(clock.now, 400)
        self.assertEqual(clock.sleeps[:4], [5, 10, 20, 40])
        self.assertLessEqual(max(clock.sleeps), er.REPORT_BACKOFF_MAX_SECONDS)

    def test_a_rejected_key_is_not_retried(self):
        answer, post, clock = self.run_report([er.ReportError("Odoo answered HTTP 403")])
        self.assertIsInstance(answer, er.ReportError)
        self.assertNotIsInstance(answer, er.OdooUnavailable)
        self.assertEqual(post.call_count, 1)
        self.assertEqual(clock.sleeps, [])

    def test_the_unit_timeout_outlasts_the_wait(self):
        here = os.path.dirname(os.path.abspath(__file__))
        with open(os.path.join(here, "hams-exception-report@.service"), encoding="utf-8") as handle:
            text = handle.read()
        line = [part for part in text.splitlines() if part.startswith("TimeoutStartSec=")][0]
        self.assertGreaterEqual(int(line.split("=")[1]), er.REPORT_ODOO_WAIT_SECONDS + 120)


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


if __name__ == "__main__":
    unittest.main()


class _RedirectProbe:
    """Two real local servers: `url` answers every request with a 302 to the second, `sink`, which records the headers of
    whatever reaches it. A credential header that follows the redirect shows up in `sink_headers`; `first_headers` holds
    what the first server received, which proves the request did carry the credential."""

    def __init__(self):
        outer = self
        self.first_headers = []
        self.sink_headers = []

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
        self._threads = [threading.Thread(target=s.serve_forever, daemon=True) for s in (self.sink, self.first)]

    def __enter__(self):
        for thread in self._threads:
            thread.start()
        return self

    def __exit__(self, *exc):
        for server in (self.first, self.sink):
            server.shutdown()
            server.server_close()


class PostReportRefusesARedirectTests(unittest.TestCase):
    def test_a_redirect_from_odoo_is_refused_and_the_key_never_reaches_the_second_host(self):
        # Tests [@ANCHOR: hams_helpdesk:daemon_post_report]
        with _RedirectProbe() as probe, mock.patch.object(er, "load_key", return_value="reporter-key"):
            with self.assertRaises(er.ReportError) as raised:
                er.post_report({"unit": "a.service"}, url=probe.url, database="db")
        self.assertIn("302", str(raised.exception))
        self.assertEqual(probe.first_headers[0]["authorization"], "Bearer reporter-key")
        self.assertEqual(probe.sink_headers, [], "the redirect target must never be asked")
