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
    sequence = fields.Integer(string="Sequence", default=10)
    hostname = fields.Char(
        string="Hostname", help="e.g. api.hams.com (leave empty to match all)"
    )
    path = fields.Char(
        string="Path", help="e.g. /adif (leave empty to match all)"
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
    def _compute_name(self):
        for route in self:
            target = route.hostname or "*"
            if route.path:
                target += route.path
            route.name = f"{target} -> {route.service_url or ''}"
