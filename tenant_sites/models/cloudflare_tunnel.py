# SPDX-License-Identifier: AGPL-3.0-or-later
from odoo import models


class CloudflareTunnel(models.Model):
    _inherit = "cloudflare.tunnel"

    # [@ANCHOR: tenant_sites:COMM_ingress_problems]
    # Verified by [@ANCHOR: tenant_sites:COMM_test_ingress_problems]
    def _ingress_problems(self, ingress):
        """Refuse a list that sends a tenant hostname to a daemon port.

        A rule without a hostname but with a path (for example ^/websocket$ -> the bus port) matches
        EVERY hostname that reaches the tunnel, tenant sites and parked domains included. While any
        tenant site exists such a rule must be scoped to a hostname."""
        problems = super()._ingress_problems(ingress)
        catch_all = ingress[-1].get("service") if ingress else None
        unscoped = [
            rule
            for rule in ingress[:-1]
            if not rule.get("hostname") and rule.get("service") != catch_all
        ]
        if not unscoped or not self.env["tenant.site.host"]._tenant_hosts_exist():
            return problems
        shown = ", ".join(f"{rule.get('path') or '*'} -> {rule['service']}" for rule in unscoped[:5])
        problems.append(
            f"{len(unscoped)} rule(s) without a hostname would answer tenant and parked domains "
            f"from other services ({shown}); give each a hostname"
        )
        return problems
