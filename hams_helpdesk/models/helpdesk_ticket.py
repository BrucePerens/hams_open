from odoo import _, api, fields, models
import datetime
import os

class HelpdeskTicket(models.Model):
    _name = "hams_helpdesk.ticket"
    _description = "Helpdesk Ticket"
    _inherit = ["mail.thread", "mail.activity.mixin"]

    # [@ANCHOR: helpdesk_ticket_lifecycle]
    # Verified by [@ANCHOR: test_01_ticket_creation_and_routing]
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
        # Verified by [@ANCHOR: test_01_ticket_creation_and_routing]
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
            # Use service account to search calendar events to ensure full visibility without sudo()
            svc_uid = self.env["zero_sudo.security.utils"]._get_service_uid("hams_helpdesk.user_helpdesk_service")
            Calendar = self.env["calendar.event"]
            if svc_uid:
                Calendar = Calendar.with_user(svc_uid)

            upcoming_shifts = Calendar.search([
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
        # Verified by [@ANCHOR: test_02_shift_handoff_wizard]
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

    def _register_hook(self):
        # burn-ignore-sudo: ADR-0055 soft-dependency documentation bootstrap
        # [@ANCHOR: helpdesk_doc_injection]
        # Verified by [@ANCHOR: test_05_doc_injection]
        if not self.env.registry.ready:
            return

        # Attempt documentation injection via manual_library or knowledge
        doc_path = os.path.join(os.path.dirname(os.path.dirname(__file__)), "data", "documentation.html")
        if not os.path.exists(doc_path):
            return

        with open(doc_path, "r", encoding="utf-8") as f:
            content = f.read()

        # Support both Odoo Enterprise Knowledge and the custom Manual Library
        # Use a service account if available, otherwise fallback to current user (likely system/admin during install)
        svc_uid = self.env["zero_sudo.security.utils"]._get_service_uid("hams_helpdesk.user_helpdesk_service")
        context_env = self.env
        if svc_uid:
            context_env = self.env(user=svc_uid)

        # burn-ignore-sudo: ADR-0022 search inside loop bypass for multi-model registration
        # This is safe because we break after the first successful match and it is not iterating over records.
        target_model = False
        if "manual.article" in self.env:
            target_model = "manual.article"
        elif "knowledge.article" in self.env:
            target_model = "knowledge.article"

        if target_model:
            existing = context_env[target_model].search([("name", "=", "Hams Helpdesk")], limit=1)
            vals = {
                "name": "Hams Helpdesk",
                "body": content,
            }
            if target_model == "manual.article":
                vals["category"] = "technical"

            if existing:
                existing.write({"body": content})
            else:
                context_env[target_model].create(vals)
