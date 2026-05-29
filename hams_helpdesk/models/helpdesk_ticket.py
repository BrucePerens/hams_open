from odoo import _, api, fields, models
from odoo.exceptions import AccessError
import datetime
import logging

_logger = logging.getLogger(__name__)

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

    website_id = fields.Many2one(
        "website",
        string="Website",
        ondelete="restrict",
        help="The website this ticket was created on.",
    )  # [@ANCHOR: helpdesk_multi_website]

    @api.model_create_multi
    def create(self, vals_list):
        # [@ANCHOR: helpdesk_ticket_creation]
        # Verified by [@ANCHOR: test_01_ticket_creation_and_routing]
        for vals in vals_list:
            if "website_id" not in vals and self.env.context.get("website_id"):
                vals["website_id"] = self.env.context.get("website_id")

        tickets = super().create(vals_list)

        # Execute automated routing and notifications using service accounts to ensure Zero-Sudo compliance
        tickets._automated_routing_and_notification()

        return tickets

    def _automated_routing_and_notification(self):
        """
        Internal automation handler for ticket assignment and notifications.
        Wraps background logic in service account contexts to bypass portal/user restrictions.
        """
        if not self:
            return

        utils = self.env["zero_sudo.security.utils"]

        # 1. Resolve On-Duty Admin via PagerDuty (if available)
        on_duty_user_id = False
        upcoming_partner_ids = []

        # Use service account if available, otherwise fallback to current env (e.g. during tests or if pager_duty not installed)
        Calendar = self.env["calendar.event"]
        try:
            pager_env = utils._get_service_env("pager_duty.user_pager_service_internal")
            Calendar = pager_env["calendar.event"]
        except AccessError as e:
            # PagerDuty service account might not be provisioned yet or module not installed.
            # This is an optional integration, so we continue with standard env.
            _logger.info("PagerDuty service env not loaded (optional integration): %s", e)

        if hasattr(Calendar, "get_current_on_duty_admin"):
            on_duty_admin = Calendar.get_current_on_duty_admin()
            if on_duty_admin:
                on_duty_user_id = on_duty_admin.id

        # Discover upcoming shifts (within 30 minutes)
        if "is_pager_duty" in Calendar._fields:
            now = fields.Datetime.now()
            thirty_mins = now + datetime.timedelta(minutes=30)
            upcoming_shifts = Calendar.search([
                ("is_pager_duty", "=", True),
                ("start", ">", now),
                ("start", "<=", thirty_mins),
            ], limit=100)
            upcoming_partner_ids = upcoming_shifts.mapped("user_id.partner_id.id")

        # 2. Apply assignments and send notifications via Helpdesk Service Account
        # We explicitly do NOT catch all exceptions here to ensure that if the service
        # account is misconfigured, we fail fast.
        hd_env = utils._get_service_env("hams_helpdesk.user_helpdesk_service")

        for ticket in self.with_env(hd_env):
            # Assignment
            if on_duty_user_id and not ticket.user_id:
                ticket.user_id = on_duty_user_id

            if ticket.user_id:
                # Email Notification
                ticket.message_post(
                    body=_("Helpdesk Ticket #%s assigned to you.") % ticket.id,
                    partner_ids=[ticket.user_id.partner_id.id],
                    subject=_("Ticket Assigned: %s") % ticket.name
                )
                # Bus Toast
                self.env["bus.bus"]._sendone(
                    ticket.user_id.partner_id,
                    "simple_notification",
                    {
                        "type": "warning",
                        "title": _("New Helpdesk Ticket"),
                        "message": _("Ticket %s requires your attention.") % ticket.name
                    }
                )

            # Pre-Shift Awareness (CC upcoming admins)
            if upcoming_partner_ids:
                current_assignee_pid = ticket.user_id.partner_id.id if ticket.user_id else False
                cc_pids = [pid for pid in upcoming_partner_ids if pid != current_assignee_pid]
                if cc_pids:
                    ticket.message_subscribe(partner_ids=cc_pids)
                    ticket.message_post(
                        body=_("Upcoming shift awareness: A new ticket was created near your shift start."),
                        partner_ids=cc_pids,
                        subject=_("Shift CC: %s") % ticket.name,
                        subtype_xmlid="mail.mt_note"
                    )

            # Ensure customer is subscribed
            if ticket.partner_id:
                ticket.message_subscribe(partner_ids=[ticket.partner_id.id])

    def write(self, vals):
        # [@ANCHOR: helpdesk_micro_privilege]
        # Micro-Privilege Security Audit: Prevent portal users from modifying restricted fields.
        if self.env.user.has_group("base.group_portal"):
            restricted_fields = {"stage", "user_id", "priority", "calendar_event_id", "website_id"}
            if any(f in vals for f in restricted_fields):
                raise AccessError(_("Portal users are not authorized to modify administrative fields."))

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
