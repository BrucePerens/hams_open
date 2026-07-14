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
        # [@ANCHOR: helpdesk_handoff_execution]

        # Verified by [@ANCHOR: test_02_shift_handoff_wizard]
        self.ensure_one()

        utils = self.env["zero_sudo.security.utils"]
        # Execute modification via service account to ensure audit trail and bypass possible write restrictions.
        # We fail fast if the service account is not properly configured.
        hd_env = utils._get_service_env("hams_helpdesk.user_helpdesk_service")
        ticket = self.ticket_id.with_env(hd_env)

        ticket.write({"user_id": self.new_user_id.id})

        old_name = self.old_user_id.name if self.old_user_id else "Unassigned"

        body = markupsafe.Markup("<b>🚨 Official Shift Handoff Executed</b><br/><br/>")
        body += markupsafe.Markup("<b>Relinquished By:</b> {}<br/>").format(old_name)
        body += markupsafe.Markup("<b>Accepted By:</b> {}<br/>").format(self.new_user_id.name)
        body += markupsafe.Markup("<b>Operator Briefing:</b><br/><i>{}</i>").format(self.handoff_notes or "")

        ticket.message_post(
            body=body,
            subject=_("Shift Handoff: %s") % ticket.name,
            partner_ids=[self.new_user_id.partner_id.id],
        )

        # Return act_window_close to explicitly close the wizard modal.
        # This gracefully drops the user back to the form view.
        return {"type": "ir.actions.act_window_close"}
