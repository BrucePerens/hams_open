# -*- coding: utf-8 -*-
# Copyright © HAMS project. AGPL-3.0-or-later.

from odoo import api, models, fields


class CloudflareTunnelRoute(models.Model):
    _name = "cloudflare.tunnel.route"
    _description = "Cloudflare Tunnel Route"
    _order = "sequence, id"

    name = fields.Char(string="Route", compute="_compute_name", store=True)
    tunnel_id = fields.Many2one(
        "cloudflare.tunnel", string="Tunnel", ondelete="cascade",
        help="If empty, this acts as a Global Route Template applied to all tunnels."
    )
    # Pushed rules are ordered by (sequence, id), and
    # cloudflare.tunnel.action_push_configuration refuses to push when
    # two rules of one tunnel's merged list (its own routes plus the
    # global templates) share a sequence -- cloudflared applies the
    # first matching rule, so the order must be explicit.
    sequence = fields.Integer(
        string="Sequence",
        default=10,
        help="Position in the pushed ingress list. Must be unique among "
        "this tunnel's routes and the global templates.",
    )
    hostname = fields.Char(
        string="Hostname", help="e.g. api.hams.com (leave empty to match all)"
    )
    # Cloudflare treats an ingress path as an unanchored regular
    # expression: "/ws" also matches "/ws/daemon_uplink".
    path = fields.Char(
        string="Path",
        help="Regular expression matched against the request path, "
        "e.g. ^/adif$ (anchor it with ^ and $ to match only that path; "
        "an unanchored /ws also matches /ws/daemon_uplink). Leave empty "
        "to match all.",
    )
    # cloudflared runs on the same host as the services it proxies to, so
    # localhost is the real, architecturally correct example below,
    # matching tunnel.py's own ingress config.
    service_url = fields.Char(
        string="Service URL",
        required=True,
        help="e.g. http://localhost:8070, tcp://localhost:22, or http_status:404"  # burn-ignore-cloudflared-ingress
    )

    @api.depends("hostname", "path", "service_url")
    # [@ANCHOR: cloudflare:COMM_tunnel_route_compute_name]
    def _compute_name(self):
        for route in self:
            target = route.hostname or "*"
            if route.path:
                target += route.path
            route.name = f"{target} -> {route.service_url or ''}"
