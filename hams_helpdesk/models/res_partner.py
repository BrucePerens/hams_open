# -*- coding: utf-8 -*-
from odoo import models, fields


class ResPartner(models.Model):
    _inherit = "res.partner"

    callsign = fields.Char(string="Callsign", help="Relevant amateur radio callsign.")
