# This software is distributed under the terms of the Affero General Public License (AGPL-3).

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
    callsign = fields.Char(
        string="Callsign", tracking=True, help="Relevant amateur radio callsign."
    )
    active = fields.Boolean(default=True)

    user_id = fields.Many2one(
        "res.users", string="Assigned To", tracking=True, index=True
    )
    partner_id = fields.Many2one(
        "res.partner", string="Customer", index=True, tracking=True
    )

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

    company_id = fields.Many2one(
        "res.company",
        string="Company",
        required=True,
        default=lambda self: self.env.company,
    )

    @api.onchange("partner_id")
    def _onchange_partner_id(self):
        if self.partner_id and self.partner_id.callsign:
            self.callsign = self.partner_id.callsign

    @api.model_create_multi
    def create(self, vals_list):
        # [@ANCHOR: helpdesk_ticket_creation]
        # Verified by [@ANCHOR: test_01_ticket_creation_and_routing]
        
        # Calculate on_duty_user_id before bulk creation
        on_duty_user_id = False
        utils = self.env["zero_sudo.security.utils"]
        Calendar = self.env["calendar.event"]
        if "is_pager_duty" in Calendar._fields:
            try:
                pager_env = utils._get_service_env("pager_duty.user_pager_service_internal")
                Calendar = pager_env["calendar.event"]
            except Exception as e: # audit-ignore-catch-all
                _logger.info("PagerDuty service env not loaded: %s", e)
            try:
                on_duty_admin = Calendar.get_current_on_duty_admin()
                if on_duty_admin:
                    on_duty_user_id = on_duty_admin.id
            except Exception as e: # audit-ignore-catch-all
                _logger.warning("Failed to get on_duty_admin: %s", e)

        # Pre-fetch records
        website_ids = {vals["website_id"] for vals in vals_list if vals.get("website_id")}
        if self.env.context.get("website_id"):
            website_ids.add(self.env.context.get("website_id"))
        partner_ids = {vals["partner_id"] for vals in vals_list if vals.get("partner_id")}
        
        websites = {w.id: w for w in self.env["website"].browse(list(website_ids)).exists()}
        partners = {p.id: p for p in self.env["res.partner"].browse(list(partner_ids)).exists()}

        for vals in vals_list:
            if "website_id" not in vals and self.env.context.get("website_id"):
                vals["website_id"] = self.env.context.get("website_id")
            if "company_id" not in vals and vals.get("website_id"):
                website = websites.get(vals["website_id"])
                if website and website.company_id:
                    vals["company_id"] = website.company_id.id
            if not vals.get("callsign") and vals.get("partner_id"):
                partner = partners.get(vals["partner_id"])
                if partner and partner.callsign:
                    vals["callsign"] = partner.callsign
            if on_duty_user_id and not vals.get("user_id"):
                vals["user_id"] = on_duty_user_id

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

        # 1. Resolve upcoming shifts for pre-shift awareness via PagerDuty (if available)
        upcoming_partner_ids = []

        # Use service account if available, otherwise fallback to current env (e.g. during tests or if pager_duty not installed)
        Calendar = self.env["calendar.event"]
        if "is_pager_duty" in Calendar._fields:
            try:
                pager_env = utils._get_service_env("pager_duty.user_pager_service_internal")
                Calendar = pager_env["calendar.event"]
            except Exception as e: # audit-ignore-catch-all
                _logger.info("PagerDuty service env not loaded (optional integration): %s", e)
            now = fields.Datetime.now()
            thirty_mins = now + datetime.timedelta(minutes=30)
            upcoming_shifts = Calendar.search(
                [
                    ("is_pager_duty", "=", True),
                    ("start", ">", now),
                    ("start", "<=", thirty_mins),
                ],
                limit=100,
            )
            upcoming_partner_ids = upcoming_shifts.mapped("user_id.partner_id.id")

        # 2. Apply assignments and send notifications via Helpdesk Service Account
        # We explicitly do NOT catch all exceptions here to ensure that if the service
        # account is misconfigured, we fail fast.
        hd_env = utils._get_service_env("hams_helpdesk.user_helpdesk_service")

        for ticket in self:
            # Switch to service account and ensure company context is correct for each ticket
            ticket_service = ticket.with_env(hd_env).with_company(ticket.company_id)
            
            # Since assignment is now handled in create(), we only check if it was assigned
            if ticket_service.user_id:
                # Email Notification
                ticket_service.message_post(
                    body=_("Helpdesk Ticket #%s assigned to you.") % ticket_service.id,
                    partner_ids=[ticket_service.user_id.partner_id.id],
                    subject=_("Ticket Assigned: %s") % ticket_service.name,
                )
                # Bus Toast
                self.env["bus.bus"]._sendone(
                    ticket_service.user_id.partner_id,
                    "simple_notification",
                    {
                        "type": "warning",
                        "title": _("New Helpdesk Ticket"),
                        "message": _("Ticket %s requires your attention.")
                        % ticket_service.name,
                    },
                )

            # Pre-Shift Awareness (CC upcoming admins)
            if upcoming_partner_ids:
                current_assignee_pid = (
                    ticket_service.user_id.partner_id.id
                    if ticket_service.user_id
                    else False
                )
                cc_pids = [
                    pid for pid in upcoming_partner_ids if pid != current_assignee_pid
                ]
                if cc_pids:
                    ticket_service.message_subscribe(partner_ids=cc_pids)
                    ticket_service.message_post(
                        body=_(
                            "Upcoming shift awareness: A new ticket was created near your shift start."
                        ),
                        partner_ids=cc_pids,
                        subject=_("Shift CC: %s") % ticket_service.name,
                        subtype_xmlid="mail.mt_note",
                    )

            # Ensure customer is subscribed
            if ticket_service.partner_id:
                ticket_service.message_subscribe(
                    partner_ids=[ticket_service.partner_id.id]
                )

    def write(self, vals):
        # [@ANCHOR: helpdesk_micro_privilege]
        # Micro-Privilege Security Audit: Prevent portal users from modifying restricted fields.
        if self.env.user.has_group("base.group_portal"):
            restricted_fields = {
                "stage",
                "user_id",
                "priority",
                "calendar_event_id",
                "website_id",
            }
            if any(f in vals for f in restricted_fields):
                raise AccessError(
                    _(
                        "Portal users are not authorized to modify administrative fields."
                    )
                )

        res = super().write(vals)
        # Mail-back facility on state change
        if "stage" in vals:
            for ticket in self:
                if ticket.partner_id:
                    stage_str = dict(self._fields["stage"]._description_selection(self.env)).get(ticket.stage)
                    ticket.message_post(
                        body=_("Your issue has been updated. New Status: %s")
                        % stage_str,
                        partner_ids=[ticket.partner_id.id],
                        subject=_("Ticket Update: %s") % ticket.name,
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
            },
        }

    def action_portal_close(self):
        """Allows portal users to close their own tickets."""
        # [@ANCHOR: helpdesk_portal_close]
        self.ensure_one()
        # Security: Ensure the user is the owner of the ticket
        if self.partner_id != self.env.user.partner_id and not self.env.user.has_group(
            "hams_helpdesk.group_helpdesk_user"
        ):
            raise AccessError(_("You are not authorized to close this ticket."))

        if self.stage != "closed":
            utils = self.env["zero_sudo.security.utils"]
            hd_env = utils._get_service_env("hams_helpdesk.user_helpdesk_service")
            self.with_env(hd_env).write({"stage": "closed"})
            self.message_post(body=_("Ticket closed by customer."))
