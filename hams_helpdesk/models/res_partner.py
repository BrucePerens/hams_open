# This software is distributed under the terms of the Affero General Public License (AGPL-3).

# -*- coding: utf-8 -*-
from odoo import api, models, fields


class ResPartner(models.Model):
    _inherit = "res.partner"

    # Found live 2026-09-08 as the non-technical-ham persona: this field was a plain,
    # never-written Char -- nothing anywhere in this codebase ever set it, yet
    # portal.py's ticket-new prefill, helpdesk_ticket.py's own onchange/create
    # fallback, AND ham_sk_workflow's account-lockout search on
    # res.users.partner_id.callsign all assumed it was populated. Every portal ticket
    # ever submitted through /my/tickets/new silently lost the submitter's real,
    # on-file callsign (ham_base's res.users.callsign, a completely separate field
    # this one happens to share a name with, never synced). Made this a real,
    # editable computed field so it tracks res.users.callsign automatically for
    # portal members while still allowing a manual value for a partner with no
    # associated login (e.g. a phone-in contact entered by a support agent) --
    # `or partner.callsign` preserves whatever was already there when there's no
    # user-side value to sync from, rather than clobbering a manual entry with False.
    callsign = fields.Char(
        string="Callsign",
        compute="_compute_callsign",
        store=True,
        readonly=False,
        help="Relevant amateur radio callsign.",
    )
    helpdesk_ticket_ids = fields.One2many("hams_helpdesk.ticket", "partner_id", string="Helpdesk Tickets")

    @api.depends("user_ids.callsign")
    def _compute_callsign(self):
        for partner in self:
            partner.callsign = (
                next((u.callsign for u in partner.user_ids if u.callsign), False) or partner.callsign
            )
