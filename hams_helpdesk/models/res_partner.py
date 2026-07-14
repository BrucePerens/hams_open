# This software is distributed under the terms of the Affero General Public License (AGPL-3).

# -*- coding: utf-8 -*-
from odoo import models, fields


class ResPartner(models.Model):
    _inherit = "res.partner"

    callsign = fields.Char(string="Callsign", help="Relevant amateur radio callsign.")
    helpdesk_ticket_ids = fields.One2many("hams_helpdesk.ticket", "partner_id", string="Helpdesk Tickets")
