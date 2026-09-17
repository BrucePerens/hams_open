# This software is distributed under the terms of the Affero General Public License (AGPL-3).

# -*- coding: utf-8 -*-
from odoo.tests.common import tagged
from odoo.addons.zero_sudo.tests.common import HamsHttpCase
from .tunnel_simulator import CloudflareTunnelSimulator


@tagged("post_install", "-at_install", "test_tunnel_daemon")
class TestCloudflareTunnelDaemon(CloudflareTunnelSimulator, HamsHttpCase):

    def test_edge_traffic_parsing(self):
        # Tests [@ANCHOR: cloudflare:COMM_simulator_setup]

        # Tests [@ANCHOR: cloudflare:COMM_simulator_teardown]

        # Tests [@ANCHOR: cloudflare:COMM_simulate_edge_request]

        # Tests [@ANCHOR: cloudflare:COMM_get_lib]

        # Tests [@ANCHOR: cloudflare:COMM_start_tunnel_simulator]

        # Tests [@ANCHOR: cloudflare:COMM_stop_tunnel_simulator]

        # [@ANCHOR: COMM_test_edge_traffic_parsing]
        """Verify that traffic from the Go simulator properly applies CF headers."""
        # The Go simulator will inject CF-Connecting-IP and CF-Visitor before hitting Odoo
        response = self.simulate_edge_request("/", cf_connecting_ip="9.9.9.9")
        self.assertEqual(response.status_code, 200)

    def test_websocket_traffic(self):
        # [@ANCHOR: COMM_test_websocket_traffic]
        """Verify that the Go simulator handles WebSocket upgrades seamlessly."""
        # Bug fix (night-watch, 2026-09-17): this used to send a raw Upgrade
        # request with no further setup and accept [101, 400, 404] as any
        # of those would prove the proxy relayed *something* through. It
        # always got RemoteDisconnected instead, which used to be filed as
        # an unexplained flake. It isn't one: Odoo's own
        # WebsocketConnectionHandler.websocket_allowed() unconditionally
        # returns False whenever odoo.modules.module.current_test is set
        # (i.e. always, in any test) -- "WebSockets are disabled during
        # tests because the test environment and the WebSocket thread use
        # the same cursor, leading to race conditions" (see
        # odoo/addons/bus/websocket.py's own comment on that method). So
        # open_connection() always raised ServiceUnavailable before this
        # fix, before ever reaching the handshake or touching the socket --
        # a status this test's own accepted set didn't even include, and
        # which apparently doesn't survive the Go proxy's relay path as a
        # clean HTTP response either. Odoo's own tour infrastructure hits
        # the identical problem and solves it the sanctioned way:
        # HttpCase.browser_js patches this exact method to return True for
        # the duration of the test (see odoo/tests/common.py, "browser_js"
        # method). Do the same here, narrowly, so this test exercises a
        # real handshake through the real proxy instead of a state Odoo
        # deliberately never produces during a test run.
        self.safe_patch(
            "odoo.addons.bus.websocket.WebsocketConnectionHandler.websocket_allowed",
            return_value=True,
        )
        response = self.simulate_edge_request(
            "/websocket",
            extra_headers={
                "Connection": "Upgrade",
                "Upgrade": "websocket",
                "Sec-WebSocket-Key": "dGhlIHNhbXBsZSBub25jZQ==",
                "Sec-WebSocket-Version": "13",
                "Origin": self.simulator_url,
            }
        )
        # A real handshake response, relayed through the Go proxy's own
        # upgrade-hijack path (net/http/httputil.ReverseProxy hijacks and
        # relays raw bytes once the backend answers 101).
        self.assertEqual(response.status_code, 101)
        
    def test_unauthorized_bypass(self):
        # [@ANCHOR: COMM_test_unauthorized_bypass]
        """Verify traffic lacking CF headers behaves as a direct access (or is rejected if strict)."""
        response = self.url_open("/")
        self.assertEqual(response.status_code, 200)

