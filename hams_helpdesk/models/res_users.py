# Copyright © Bruce Perens K6BP.
# SPDX-License-Identifier: AGPL-3.0-or-later

# -*- coding: utf-8 -*-
"""`res.users`: the helpdesk tickets a member filed, in their GDPR data export.

Export only. What erasure should do with tickets (a thread with staff replies, child-safety reports held for the law)
is an open policy question: night_shift_todo/medium/helpdesk-tickets-on-member-erasure-*.md in hams_com. Child-safety
reports (type csam_enticement_trafficking, or any ticket that names a reported user or a recording) are never
exported to the account they concern.
"""
from odoo import fields, models

GDPR_SERVICE = "zero_sudo.gdpr_service_internal"
EXPORT_LIMIT = 1000


# [@ANCHOR: hams_helpdesk:res_users_helpdesk]
class ResUsersHelpdesk(models.Model):
    _inherit = "res.users"

    # [@ANCHOR: hams_helpdesk:res_users_get_gdpr_export_data]
    # Verified by [@ANCHOR: hams_helpdesk:test_gdpr_export_is_own_tickets_only]
    def _get_gdpr_export_data(self):
        """Adds `helpdesk_tickets`: tickets whose customer is this member's partner (subject, description, callsign,
        type, stage, creation time). Staff notes, triage fields and the thread are not included, and neither is any
        child-safety report."""
        self.ensure_one()
        data = super()._get_gdpr_export_data()
        svc_uid = self.env["zero_sudo.security.utils"]._get_service_uid(GDPR_SERVICE)
        tickets = self.env["hams_helpdesk.ticket"].with_user(svc_uid).search(
            [("partner_id", "=", self.partner_id.id), ("ticket_type", "!=", "csam_enticement_trafficking"),
             ("ncmec_reported_user_id", "=", False), ("ncmec_recording_uuid", "=", False)],
            limit=EXPORT_LIMIT)
        if tickets:
            data["helpdesk_tickets"] = [
                {"subject": t.name, "description": t.description, "callsign": t.callsign,
                 "ticket_type": t.ticket_type, "stage": t.stage,
                 "created": fields.Datetime.to_string(t.create_date)}
                for t in tickets
            ]
        return data
