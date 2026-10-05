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
        problems.extend(self._static_rule_problems(ingress))
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

    # [@ANCHOR: tenant_sites:COMM_static_rule_problems]
    # Verified by [@ANCHOR: tenant_sites:COMM_test_static_rule_guard]
    def _static_rule_problems(self, ingress):
        """Refuse a list that drops a tenant's explicit `/static/` rule.

        The catch-all answers any tenant hostname from Odoo, so losing a rule can no longer strand a
        site, but a site whose `/static/` is served by a separate file server (`tenant.site.static_service`,
        for example perens.com on http://localhost:18201) would then have its files sent to Odoo, which
        answers 404. Every active site with a static service needs, for every one of its hostnames, a rule
        with that hostname, the path ^/static/ and that service. Rows deleted by hand in the Cloudflare
        panel are the way this happens, and a push must not publish the loss. (Decided by the coordinator
        on Bruce's instruction, 2026-10-05, option (b) of tunnel-guard-for-tenant-static-rules-b83d6e15.)"""
        service_env = self.env["tenant.site.host"]._service_env()
        sites = service_env["tenant.site"].search(
            [("active", "=", True), ("static_service", "!=", False)], limit=1000
        )
        present = {
            (rule.get("hostname"), rule.get("path"), rule.get("service"))
            for rule in ingress
            if rule.get("hostname")
        }
        problems = []
        for site in sites:
            wanted = (site.static_service or "").strip()
            if not wanted:
                continue
            missing = sorted(
                host.name for host in site.host_ids if (host.name, "^/static/", wanted) not in present
            )
            if missing:
                problems.append(
                    f"tenant site {site.name} serves /static/ from {wanted}, but no rule sends ^/static/ there "
                    f"for {', '.join(missing)}; restore the route(s) (or clear the site's Static file service)"
                )
        return problems
