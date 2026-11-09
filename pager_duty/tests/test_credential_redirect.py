# SPDX-License-Identifier: AGPL-3.0-or-later
"""The pager daemons that send a credential refuse a redirect, so the key never follows a 3xx answer to another host.

Each test starts two real local servers (the first answers 302 to the second) and asserts the second never receives the
credential header. urllib used to copy every request header to the redirect target.
"""

import importlib
import threading
from http.server import BaseHTTPRequestHandler, HTTPServer

from odoo.tests.common import tagged
from odoo.addons.zero_sudo.tests.common import HamsTransactionCase
from odoo.addons.pager_duty.daemon import check_github_pat_expiry as check_mod
from odoo.addons.pager_duty.daemon import generalized_monitor


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


@tagged("post_install", "-at_install")
class TestPagerDaemonsRefuseRedirects(HamsTransactionCase):
    def test_the_monitor_odoo_client_refuses_a_redirect(self):
        # Tests [@ANCHOR: pager_duty:odoo_client_execute]
        with _RedirectProbe() as probe:
            client = generalized_monitor.OdooClient(probe.url, "db", "pager_service_internal", "monitor-key")
            with self.assertRaises(Exception) as raised:
                client.execute("pager.check", "check_heartbeat_rpc", hb_uuid="x")
        self.assertIn("302", str(raised.exception))
        self.assertEqual(probe.first_headers[0]["authorization"], "bearer monitor-key")
        self.assertEqual(probe.sink_headers, [], "the redirect target must never be asked")

    def test_the_mcp_odoo_client_refuses_a_redirect(self):
        # Tests [@ANCHOR: pager_duty:mcp_odoo_client_execute]
        pager_mcp_server = importlib.import_module("odoo.addons.pager_duty.daemon.pager_mcp_server")
        with _RedirectProbe() as probe:
            client = pager_mcp_server.OdooClient(probe.url, "db", "mcp-key")
            with self.assertRaises(RuntimeError) as raised:
                client.execute("pager.incident", "mcp_get_incident_detail", ids=[7])
        self.assertIn("302", str(raised.exception))
        self.assertEqual(probe.first_headers[0]["authorization"], "bearer mcp-key")
        self.assertEqual(probe.sink_headers, [], "the redirect target must never be asked")

    def test_the_github_pat_expiry_check_refuses_a_redirect(self):
        # Tests [@ANCHOR: pager_duty:fetch_github_pat_expiry]
        with _RedirectProbe() as probe:
            self.safe_patch("odoo.addons.pager_duty.daemon.check_github_pat_expiry.GITHUB_USER_URL", probe.url + "/user")
            with self.assertRaises(Exception) as raised:
                check_mod._fetch_expiry_header("ghp_test_token")
        self.assertIn("302", str(raised.exception))
        self.assertEqual(probe.first_headers[0]["authorization"], "Bearer ghp_test_token")
        self.assertEqual(probe.sink_headers, [], "the redirect target must never be asked")
