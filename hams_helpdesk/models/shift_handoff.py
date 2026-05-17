from odoo import _, fields, models
import logging

_logger = logging.getLogger(__name__)

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

        utils = self.env["zero_sudo.security.utils"]
        try:
            # Execute modification via service account to ensure audit trail and bypass possible write restrictions
            hd_env = utils._get_service_env("hams_helpdesk.user_helpdesk_service")
            ticket = self.ticket_id.with_env(hd_env)
        except Exception as e: # audit-ignore-catch-all
            _logger.warning("Failed to resolve helpdesk service env for handoff execution: %s", e)
            ticket = self.ticket_id

        ticket.write({"user_id": self.new_user_id.id})

        old_name = self.old_user_id.name if self.old_user_id else "Unassigned"

        body = "<b>🚨 Official Shift Handoff Executed</b><br/><br/>"
        body += f"<b>Relinquished By:</b> {old_name}<br/>"
        body += f"<b>Accepted By:</b> {self.new_user_id.name}<br/>"
        body += f"<b>Operator Briefing:</b><br/><i>{self.handoff_notes}</i>"

        ticket.message_post(
            body=body,
            subject=_("Shift Handoff: %s") % ticket.name,
            partner_ids=[self.new_user_id.partner_id.id]
        )
        return {"type": "ir.actions.act_window_close"}
