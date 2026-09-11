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
    # [@ANCHOR: hams_helpdesk:res_partner_compute_callsign]
    def _compute_callsign(self):
        # Bug-hunt fix, 2026-09-11: this is a `store=True` computed field on the BASE res.partner
        # model, so Odoo may need to flush its pending recomputation during ANY write() to ANY
        # partner record, under whatever user happens to be making that write -- not just a real
        # portal/internal user reading their own chatter. `partner.user_ids` reads `res.users`
        # under the CALLING user's own ACLs, which a narrowly-scoped service account (by this
        # codebase's own zero-sudo design) typically does not have at all. Confirmed live: writing
        # `res.partner.membership_state` via ham_club_management's Stripe-webhook service account
        # (which has zero res.users grant) raised a real AccessError -- "Failed to read field
        # res.partner.user_ids" -- with nothing in ham_club_management itself touching user_ids,
        # because this field's own pending compute (from an earlier create() with no explicit
        # callsign) was what actually triggered the read. Routed through
        # zero_sudo.odoo_facility_service_internal (this codebase's own established "ground truth,
        # safe for any caller" account, already a base.group_user member and therefore covered by
        # Odoo's own base_res_users_employee ACL) so this internal consistency computation no
        # longer depends on the calling user's own res.users access.
        facility_uid = self.env["zero_sudo.security.utils"]._get_service_uid(
            "zero_sudo.odoo_facility_service_internal"
        )
        for partner in self:
            users = partner.with_user(facility_uid).user_ids
            partner.callsign = (
                next((u.callsign for u in users if u.callsign), False) or partner.callsign
            )
