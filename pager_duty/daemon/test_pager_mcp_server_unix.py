#!/usr/bin/env python3
# SPDX-License-Identifier: AGPL-3.0-or-later
# -*- coding: utf-8 -*-
"""The pager MCP server's Odoo client speaks to the RPC guard's Unix socket when ODOO_URL is unix:///path."""

import os
import socketserver
import sys
import tempfile
import threading
import unittest
from http.server import BaseHTTPRequestHandler

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import pager_mcp_server as module  # noqa: E402


class Guard(BaseHTTPRequestHandler):
    """The RPC guard's Unix socket stand-in: records (path, Host, Authorization, body) and answers `[{"id": 7}]`."""
    seen = []

    def do_POST(self):
        Guard.seen.append((self.path, self.headers["Host"], self.headers["Authorization"], self.rfile.read(int(self.headers["Content-Length"]))))
        body = b'[{"id": 7}]'
        self.send_response(200)
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def log_message(self, *args):
        pass


class UnixSocketTests(unittest.TestCase):
    def setUp(self):
        Guard.seen = []
        directory = tempfile.TemporaryDirectory()
        self.addCleanup(directory.cleanup)
        self.socket_path = os.path.join(directory.name, "rpc.sock")
        server = socketserver.UnixStreamServer(self.socket_path, Guard)
        thread = threading.Thread(target=server.serve_forever, daemon=True)  # burn-ignore-test-daemon-thread: hermetic fixture, shut down in cleanup
        thread.start()
        self.addCleanup(server.server_close)
        self.addCleanup(server.shutdown)

    # [@ANCHOR: pager_duty:test_mcp_a_unix_url_reaches_odoo_over_the_socket]
    def test_mcp_a_unix_url_reaches_odoo_over_the_socket(self):
        # Tests [@ANCHOR: pager_duty:unix_opener_mcp]
        # Tests [@ANCHOR: pager_duty:mcp_odoo_client_execute]
        client = module.OdooClient("unix://" + self.socket_path, "db", "secret-key")
        self.assertEqual(client.execute("pager.incident", "search_read", limit=1), [{"id": 7}])
        self.assertEqual(Guard.seen, [("/json/2/pager.incident/search_read", "unix-socket", "bearer secret-key", b'{"limit": 1}')])


if __name__ == "__main__":
    unittest.main()
