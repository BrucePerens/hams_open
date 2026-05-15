from odoo import _, fields, models

class ShiftHandoffWizard(models.TransientModel):
    _name = "hams_helpdesk.shift_handoff"
    _description = "Shift Handoff Wizard"

    ticket_id = fields.Many2one("hams_helpdesk.ticket", string="Ticket", required=True)
    old_user_id = fields.Many2one("res.users", string="Current Assignee", readonly=True)
    new_user_id = fields.Many2one("res.users", string="Next Shift Assignee", required=True)
    handoff_notes = fields.Text(string="Handoff Notes", required=True, help="Detailed context for the incoming operator.")

    def action_confirm_handoff(self):
        # [@ANCHOR: helpdesk_handoff_execution]
        # Verified by [@ANCHOR: test_02_shift_handoff_wizard]
        self.ensure_one()

        self.ticket_id.write({"user_id": self.new_user_id.id})

        old_name = self.old_user_id.name if self.old_user_id else "Unassigned"

        body = "<b>🚨 Official Shift Handoff Executed</b><br/><br/>"
        body += f"<b>Relinquished By:</b> {old_name}<br/>"
        body += f"<b>Accepted By:</b> {self.new_user_id.name}<br/>"
        body += f"<b>Operator Briefing:</b><br/><i>{self.handoff_notes}</i>"

        self.ticket_id.message_post(
            body=body,
            subject=_("Shift Handoff: %s") % self.ticket_id.name,
            partner_ids=[self.new_user_id.partner_id.id]
        )
        return {"type": "ir.actions.act_window_close"}
