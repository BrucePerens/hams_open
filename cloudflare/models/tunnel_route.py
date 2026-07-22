# -*- coding: utf-8 -*-
# Copyright © HAMS project. AGPL-3.0.

from odoo import models, fields

class CloudflareTunnelRoute(models.Model):
    _name = "cloudflare.tunnel.route"
    _description = "Cloudflare Tunnel Route"
    _order = "sequence, id"

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
    service_url = fields.Char(
        string="Service URL", 
        required=True, 
        help="e.g. http://localhost:8070, tcp://localhost:22, or http_status:404"
    )
