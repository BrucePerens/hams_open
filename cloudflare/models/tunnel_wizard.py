# -*- coding: utf-8 -*-
# Copyright © HAMS project. AGPL-3.0.
from odoo import models, fields


class CloudflareTunnelWizard(models.TransientModel):
    _name = "cloudflare.tunnel.wizard"
    _description = "Cloudflare Tunnel Setup Wizard"
    name = fields.Char(string="Name", default=lambda self: self._description)

    command = fields.Text(string="Installation Command", readonly=True, help="Copy and execute this command in your terminal to install the Cloudflare Tunnel.")
