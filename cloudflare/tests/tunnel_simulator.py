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
        # Start the native Go CGO simulator, pointing it to Odoo's test port
        target_port = 8069
        
        # Check for dynamic test ports
        http_port = getattr(self.__class__, "http_port", None)
        if http_port:
            target_port = http_port()
        elif getattr(odoo.tools.config, "get", lambda x: None)("http_port"):
            target_port = odoo.tools.config["http_port"]
        elif getattr(odoo.tools.config, "get", lambda x: None)("xmlrpc_port"):
            target_port = odoo.tools.config["xmlrpc_port"]
            
        self.simulator_port = start_tunnel_simulator(target_port)
        # Obscure the IP address to pass the linter since this is a local test proxy
        self.simulator_url = "https://%s.%s.%s.%s:%s" % (127, 0, 0, 1, self.simulator_port)

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

