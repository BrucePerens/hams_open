# This software is distributed under the terms of the Affero General Public License (AGPL-3).

# -*- coding: utf-8 -*-
from odoo.addons.cloudflare.utils.cloudflare_daemon import start_tunnel_simulator, stop_tunnel_simulator
import ssl
import urllib.request
import json
import logging
import odoo

_logger = logging.getLogger(__name__)



class CloudflareTunnelSimulator:
    
    def setUp(self):
        super().setUp()
        # Start the native Go CGO simulator, pointing it to Odoo's test
        # port. self.http_port() is a real, guaranteed classmethod on
        # Odoo's own HttpCase (this mixin is only ever combined with an
        # HttpCase-derived test class, since it needs a real running
        # server to point the simulator at) -- matches the same,
        # established, simpler fallback chain zero_sudo/tests/common.py
        # already uses (cls.http_port() or odoo.tools.config["xmlrpc_port"]),
        # rather than the 3-argument getattr() probing this used to do,
        # which masked the fact that http_port() is never actually optional.
        target_port = self.http_port() or odoo.tools.config.get("xmlrpc_port") or 8069

        self.simulator_port = start_tunnel_simulator(target_port)
        # A real, honest local test-proxy loopback address --
        # burn-ignore-self-hosted-server: this simulator is a Go CGO
        # process spawned by start_tunnel_simulator() moments earlier in
        # the same process tree, not a different container's service.
        self.simulator_url = "https://127.0.0.1:%s" % self.simulator_port  # burn-ignore-self-hosted-server

    def tearDown(self):
        super().tearDown()
        stop_tunnel_simulator()

    def simulate_edge_request(self, path, cf_connecting_ip='1.2.3.4', cf_visitor='{"scheme":"https"}', extra_headers=None):
        """
        Sends an HTTPS request directly to the Go CGO Simulator, which will proxy
        it to Odoo and inject the Cloudflare edge headers.
        """
        url = f"{self.simulator_url}{path}"
        headers = {
            'CF-Connecting-IP': cf_connecting_ip,
            'CF-Visitor': cf_visitor,
            'X-Forwarded-For': cf_connecting_ip,
        }
        if extra_headers:
            headers.update(extra_headers)
            
        # Use Odoo's native url_open which automatically handles test session cookies
        # and test framework requirements.
        response = self.url_open(url, headers=headers, timeout=10)
        return response

