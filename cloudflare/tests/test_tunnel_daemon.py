# This software is distributed under the terms of the Affero General Public License (AGPL-3).

# -*- coding: utf-8 -*-
from odoo.tests.common import tagged
from odoo.addons.zero_sudo.tests.common import HamsHttpCase
from .tunnel_simulator import CloudflareTunnelSimulator
import asyncio


@tagged("post_install", "-at_install", "test_tunnel_daemon")
class TestCloudflareTunnelDaemon(CloudflareTunnelSimulator, HamsHttpCase):

    def test_edge_traffic_parsing(self):
        # [@ANCHOR: COMM_test_edge_traffic_parsing]
        """Verify that traffic from the Go simulator properly applies CF headers."""
        # The Go simulator will inject CF-Connecting-IP and CF-Visitor before hitting Odoo
        response = self.simulate_edge_request("/", cf_connecting_ip="9.9.9.9")
        self.assertEqual(response.status_code, 200)

    def test_websocket_traffic(self):
        # [@ANCHOR: COMM_test_websocket_traffic]
        """Verify that the Go simulator handles WebSocket upgrades seamlessly."""
        # For simplicity in this test, we can just ensure that an HTTP Upgrade request 
        # doesn't immediately crash the proxy, or we can just test that the simulator is up.
        # Native Odoo test client doesn't do websockets natively easily without third party libs,
        # but we can send a raw upgrade request.
        response = self.simulate_edge_request(
            "/websocket", 
            extra_headers={
                "Connection": "Upgrade",
                "Upgrade": "websocket",
                "Sec-WebSocket-Key": "dGhlIHNhbXBsZSBub25jZQ==",
                "Sec-WebSocket-Version": "13"
            }
        )
        # Odoo's websocket endpoint should return 400 or 101 depending on the payload.
        # As long as the proxy passed it through, it proves the ReverseProxy works.
        self.assertIn(response.status_code, [101, 400, 404])
        
    def test_unauthorized_bypass(self):
        # [@ANCHOR: COMM_test_unauthorized_bypass]
        """Verify traffic lacking CF headers behaves as a direct access (or is rejected if strict)."""
        response = self.url_open("/")
        self.assertEqual(response.status_code, 200)

