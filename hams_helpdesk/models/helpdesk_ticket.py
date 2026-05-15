from odoo import _, api, fields, models
import datetime

class HelpdeskTicket(models.Model):
    _name = "hams_helpdesk.ticket"
    _description = "Helpdesk Ticket"
    _inherit = ["mail.thread", "mail.activity.mixin"]

    # [@ANCHOR: helpdesk_ticket_lifecycle]
    name = fields.Char(string="Subject", required=True, tracking=True)
    description = fields.Html(string="Description")
    active = fields.Boolean(default=True)

    user_id = fields.Many2one(
        "res.users", string="Assigned To", tracking=True, index=True
    )
    partner_id = fields.Many2one("res.partner", string="Customer", index=True, tracking=True)

    stage = fields.Selection(
        [
            ("new", "New"),
            ("in_progress", "In Progress"),
            ("resolved", "Resolved"),
            ("closed", "Closed"),
        ],
        string="Stage",
        default="new",
        required=True,
        tracking=True,
    )

    priority = fields.Selection(
        [
            ("0", "Low"),
            ("1", "Medium"),
            ("2", "High"),
            ("3", "Critical"),
        ],
        string="Priority",
        default="0",
        tracking=True,
    )

    calendar_event_id = fields.Many2one(
        "calendar.event",
        string="Scheduled Event",
        help="Linked calendar event for incident response or scheduled assistance.",
    )

    @api.model_create_multi
    def create(self, vals_list):
        # [@ANCHOR: helpdesk_ticket_creation]
        tickets = super().create(vals_list)

        # PagerDuty API Integration: Resolve on-duty personnel
        on_duty_user = False
        if hasattr(self.env["calendar.event"], "get_current_on_duty_admin"):
            on_duty_user = self.env["calendar.event"].get_current_on_duty_admin()

        # Discover upcoming shifts (within 30 minutes)
        now = fields.Datetime.now()
        thirty_mins = now + datetime.timedelta(minutes=30)
        upcoming_users = self.env["res.users"]
        if "is_pager_duty" in self.env["calendar.event"]._fields:
            upcoming_shifts = self.env["calendar.event"].with_user(self.env.user.id).search([
                ("is_pager_duty", "=", True),
                ("start", ">", now),
                ("start", "<=", thirty_mins),
            ], limit=100)
            upcoming_users = upcoming_shifts.mapped("user_id")

        for ticket in tickets:
            if on_duty_user and not ticket.user_id:
                ticket.user_id = on_duty_user.id

            if ticket.user_id:
                # 1. Email Notification
                ticket.message_post(
                    body=_("Helpdesk Ticket #%s assigned to you.") % ticket.id,
                    partner_ids=[ticket.user_id.partner_id.id],
                    subject=_("Ticket Assigned: %s") % ticket.name
                )
                # 2. Toast Notification via Odoo Bus
                self.env["bus.bus"]._sendone(
                    ticket.user_id.partner_id,
                    "simple_notification",
                    {
                        "type": "warning",
                        "title": _("New Helpdesk Ticket"),
                        "message": _("Ticket %s requires your attention.") % ticket.name
                    }
                )

            # Pre-Shift CC Logic (Silent notification)
            for up_user in upcoming_users:
                if up_user != ticket.user_id:
                    ticket.message_subscribe(partner_ids=[up_user.partner_id.id])
                    ticket.message_post(
                        body=_("FYI: A new issue was logged shortly before your shift begins."),
                        partner_ids=[up_user.partner_id.id],
                        subject=_("Upcoming Shift CC: %s") % ticket.name,
                        subtype_xmlid="mail.mt_note"  # Prevents actual paging
                    )

            # Ensure customer is subscribed for mail-back loop
            if ticket.partner_id:
                ticket.message_subscribe(partner_ids=[ticket.partner_id.id])

        return tickets

    def write(self, vals):
        res = super().write(vals)
        # Mail-back facility on state change
        if "stage" in vals:
            for ticket in self:
                if ticket.partner_id:
                    stage_str = dict(self._fields["stage"].selection).get(ticket.stage)
                    ticket.message_post(
                        body=_("Your issue has been updated. New Status: %s") % stage_str,
                        partner_ids=[ticket.partner_id.id],
                        subject=_("Ticket Update: %s") % ticket.name
                    )
        return res

    def action_shift_handoff(self):
        """Opens the formal shift handoff wizard."""
        # [@ANCHOR: helpdesk_shift_handoff]
        self.ensure_one()
        return {
            "name": "Formal Shift Handoff",
            "type": "ir.actions.act_window",
            "res_model": "hams_helpdesk.shift_handoff",
            "view_mode": "form",
            "target": "new",
            "context": {
                "default_ticket_id": self.id,
                "default_old_user_id": self.user_id.id if self.user_id else False,
            }
        }
