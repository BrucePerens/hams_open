# This software is distributed under the terms of the Affero General Public License (AGPL-3).

from odoo import _, fields, models
import markupsafe


class ShiftHandoffWizard(models.TransientModel):
    _name = "hams_helpdesk.shift_handoff"
    _description = "Shift Handoff Wizard"
    name = fields.Char(string="Name", default=lambda self: self._description)

    ticket_id = fields.Many2one("hams_helpdesk.ticket", string="Ticket", required=True)
    old_user_id = fields.Many2one("res.users", string="Current Assignee", readonly=True)
    new_user_id = fields.Many2one(
        "res.users", string="Next Shift Assignee", required=True
    )
    handoff_notes = fields.Text(
        string="Handoff Notes",
        required=True,
        help="Detailed context for the incoming operator.",
    )

    def action_confirm_handoff(self):
        # [@ANCHOR: COMM_helpdesk_handoff_execution]

        # # Verified by [@ANCHOR: COMM_test_02_shift_handoff_wizard]
        self.ensure_one()

        utils = self.env["zero_sudo.security.utils"]
        # Execute modification via service account to ensure audit trail and bypass possible write restrictions.
        # We fail fast if the service account is not properly configured.
        hd_env = utils._get_service_env("hams_helpdesk.user_helpdesk_service")
        ticket = self.ticket_id.with_env(hd_env)

        ticket.with_context(mail_notrack=True).write({"user_id": self.new_user_id.id})

        old_name = self.old_user_id.name if self.old_user_id else "Unassigned"

        body = markupsafe.Markup("<b>🚨 Official Shift Handoff Executed</b><br/><br/>")
        body += markupsafe.Markup("<b>Relinquished By:</b> {}<br/>").format(old_name)
        body += markupsafe.Markup("<b>Accepted By:</b> {}<br/>").format(self.new_user_id.name)
        body += markupsafe.Markup("<b>Operator Briefing:</b><br/><i>{}</i>").format(self.handoff_notes or "")

        # Bug-hunt fix (2026-09-09): this operator briefing (who relinquished/
        # accepted the ticket, plus the free-text handoff_notes an agent
        # writes for the *next* internal operator) was posted with no
        # subtype_xmlid, which defaults to mail.mt_comment -- a
        # customer-visible subtype. portal/controllers/portal_thread.py's
        # own /mail/chatter_fetch route ("All users in the portal see only
        # non-internal messages") only excludes messages whose subtype is
        # internal (mail.mt_note), so every shift handoff -- including any
        # internal-only operational context in handoff_notes -- was showing
        # up verbatim in the customer's own /my/ticket/<id> "Communication
        # History". mail.mt_note keeps this posted (and the new assignee
        # still gets notified via partner_ids) while excluding it from the
        # portal-facing thread.
        ticket.message_post(
            body=body,
            subject=_("Shift Handoff: %s") % ticket.name,
            partner_ids=[self.new_user_id.partner_id.id],
            subtype_xmlid="mail.mt_note",
        )

        # Return act_window_close to explicitly close the wizard modal.
        # This gracefully drops the user back to the form view.
        return {"type": "ir.actions.act_window_close"}
