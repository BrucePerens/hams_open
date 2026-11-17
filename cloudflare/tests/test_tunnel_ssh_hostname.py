# Copyright © Bruce Perens K6BP.
# SPDX-License-Identifier: AGPL-3.0-or-later
from odoo.addons.zero_sudo.tests.common import HamsTransactionCase
from odoo.tests.common import tagged


@tagged("post_install", "-at_install")
class TestTunnelSshHostname(HamsTransactionCase):
    def _ssh_hostnames(self, domain):
        website = self.env["website"].create({"name": "Ssh Host Site", "domain": domain})
        tunnel = self.env["cloudflare.tunnel"].create(
            {"cf_tunnel_id": "ssh-host-tunnel", "name": "t", "website_id": website.id, "ssh_route_enabled": True}
        )
        return [rule["hostname"] for rule in tunnel._build_ingress() if rule["service"].startswith("ssh://")]

    def test_the_ssh_host_name_has_no_port(self):
        # Tests [@ANCHOR: cloudflare:COMM_tunnel_build_ingress]
        # ssh.<netloc> gave "ssh.example.com:8069" for a website domain with a port, which Cloudflare refuses.
        self.assertEqual(self._ssh_hostnames("https://ssh-port.example.com:8069"), ["ssh.ssh-port.example.com"])
        self.assertEqual(self._ssh_hostnames("https://ssh-plain.example.com"), ["ssh.ssh-plain.example.com"])
