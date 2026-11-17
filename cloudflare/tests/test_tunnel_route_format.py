# Copyright © Bruce Perens K6BP.
# SPDX-License-Identifier: AGPL-3.0-or-later
from odoo.addons.zero_sudo.tests.common import HamsTransactionCase
from odoo.exceptions import ValidationError
from odoo.tests.common import tagged


@tagged("post_install", "-at_install")
class TestTunnelRouteFormat(HamsTransactionCase):
    def _make(self, **vals):
        return self.env["cloudflare.tunnel.route"].create(dict({"service_url": "http://odoo:8070"}, **vals))

    # [@ANCHOR: cloudflare:COMM_test_tunnel_route_format]
    def test_a_malformed_route_is_refused_when_it_is_saved(self):
        # Tests [@ANCHOR: cloudflare:COMM_tunnel_route_format]
        # Cloudflare refuses the whole push for any of these, long after the route was saved.
        bad = (
            {"hostname": "has space.example.com"},
            {"hostname": "https://api.example.com"},
            {"hostname": "-bad.example.com"},
            {"hostname": "a" * 64 + ".example.com"},
            {"path": "^/(unclosed"},
            {"path": "[a-"},
            {"service_url": "odoo:8070"},
            {"service_url": "gopher://odoo:70"},
            {"service_url": "http://has space"},
            {"service_url": "http_status:40"},
        )
        for vals in bad:
            with self.subTest(vals=vals):
                with self.assertRaises(ValidationError), self.env.cr.savepoint():
                    self._make(**vals)
                    self.env.flush_all()

    def test_well_formed_routes_are_stored(self):
        # Tests [@ANCHOR: cloudflare:COMM_tunnel_route_format]
        self._make(hostname="api.example.com", path="^/adif$")
        self._make(hostname="*.example.com")
        self._make(service_url="http_status:404")
        self._make(service_url="tcp://odoo:22")
        self._make(service_url="ws://relay-bridge:8766")
        self._make()
