# Copyright © Bruce Perens K6BP.
# SPDX-License-Identifier: AGPL-3.0-or-later

from odoo import _, models

from .incident_ticket_adapter import _is_inbound_email_source


# [@ANCHOR: pager_duty:helpdesk_ticket_triage_cleared]
class HelpdeskTicket(models.Model):
    _inherit = "hams_helpdesk.ticket"

    # [@ANCHOR: pager_duty:helpdesk_ticket_triage_cleared_method]
    # Verified by [@ANCHOR: test_01_a_resolved_incident_retires_its_new_ticket_with_a_note]
    def _triage_cleared(self):
        """Adds the tickets a pager incident created, once that incident is resolved, to the automatic tickets that
        hams_helpdesk retires before notifying anyone (``_triage_retire_stale``). A ticket from inbound mail is a person's
        message, never retired this way."""
        cleared = super()._triage_cleared()
        pd_uid = self.env["zero_sudo.security.utils"]._get_service_uid("pager_duty.user_pager_service_internal")
        incidents = self.env["pager.incident"].with_user(pd_uid).search(
            [
                ("helpdesk_ticket_model", "=", "hams_helpdesk.ticket"),
                ("helpdesk_ticket_id", "in", self.ids),
                ("status", "=", "resolved"),
            ]
        )
        # Several incidents can share one ticket (joined unit tickets): it clears only when none of them is still unresolved.
        still_open = set(
            self.env["pager.incident"]
            .with_user(pd_uid)
            .search(
                [
                    ("helpdesk_ticket_model", "=", "hams_helpdesk.ticket"),
                    ("helpdesk_ticket_id", "in", incidents.mapped("helpdesk_ticket_id")),
                    ("status", "!=", "resolved"),
                ]
            )
            .mapped("helpdesk_ticket_id")
        )
        for incident in incidents.filtered(
            lambda i: not _is_inbound_email_source(i.source) and i.helpdesk_ticket_id not in still_open
        ):
            cleared.setdefault(incident.helpdesk_ticket_id, _("The pager incident %s is resolved.") % incident.name)
        return cleared
