# This software is distributed under the terms of the Affero General Public License (AGPL-3).

from odoo import _, api, fields, models
from odoo.exceptions import AccessError, UserError
from odoo.addons.zero_sudo.daemon.ssrf_safe_fetch import (
    SSRFValidationError,
    urlopen_ssrf_safe as _urlopen_ssrf_safe,
)
import base64
import logging
import urllib.error
import urllib.request
import xml.etree.ElementTree as ET

_logger = logging.getLogger(__name__)

# docs/proposals/CHILD_SAFETY_COMMUNICATIONS_CONSENT.md, section G / Phase 8: the mandatory
# 18 U.S.C. Sec. 2258A (REPORT Act) reporting workflow. This ticket_type value is the "distinct,
# faster-SLA... routing for apparent CSAM/enticement/trafficking flags" the proposal calls for --
# see _CSAM_TICKET_TYPE's own use in create()/_ncmec_report_ticket_for_recording below for what
# "faster-SLA" actually means here: forced Critical priority plus an immediate, unconditional
# notification to every helpdesk manager (not just whoever happens to be on duty), on top of the
# ordinary on-duty routing every other ticket already gets from _automated_routing_and_notification.
_CSAM_TICKET_TYPE = "csam_enticement_trafficking"

# ham_communications_consent (hams_com, proprietary) is NOT a dependency of this module --
# hams_helpdesk lives in hams_open and must remain installable/testable standalone (see
# pager_duty/models/pager_check.py's own update_lets_encrypt_domains() for the established
# precedent of this exact "open depends on proprietary" direction being structurally impossible:
# a real __manifest__.py 'depends' entry here would mean hams_open could no longer be installed
# without also checking out hams_com, the first such exception in either manifest graph).
# comm_consent_qso_recording.py's own model name, referenced here only as a string constant so a
# presence check (`in self.env`, see _ncmec_apply_recording_legal_hold_best_effort below) can work
# whether or not that module happens to be installed alongside this one.
_QSO_RECORDING_MODEL = "ham.simulated.band.qso.recording"

# zero_sudo/models/security_utils.py's own read whitelist (_get_param_read_whitelist) must list
# every one of these five keys -- see that file's own new entries and their comment for why.
# Unlike a Stripe secret key (ham_base._SERVICE_ALLOWED_KEYS, hams_com-only), hams_open has no
# per-service-account-scoped secret-parameter mechanism of its own (confirmed directly: no
# ir_config_parameter.py override exists anywhere under hams_open) -- zero_sudo's own whitelist
# IS the whole gate here, exactly the same shape already used for pager_duty.domain_api_identity.
_NCMEC_PARAM_BASE_URL = "hams_helpdesk.ncmec_api_base_url"
_NCMEC_PARAM_USERNAME = "hams_helpdesk.ncmec_api_username"
_NCMEC_PARAM_PASSWORD = "hams_helpdesk.ncmec_api_password"
_NCMEC_PARAM_CONTACT_EMAIL = "hams_helpdesk.ncmec_contact_email"
_NCMEC_PARAM_CONTACT_PHONE = "hams_helpdesk.ncmec_contact_phone"

# NCMEC's real CyberTipline Reporting API (confirmed directly against NCMEC's own published
# technical documentation, https://report.cybertip.org/ispws/documentation, 2026-09-23): HTTP
# Basic Auth over HTTPS, XML payloads, credentials "must be requested from and supplied by NCMEC"
# (no self-service registration found) -- hams.com does not have real credentials as of this
# writing (docs/proposals/CHILD_SAFETY_COMMUNICATIONS_CONSENT.md's own "Answered, this pass"
# section leaves "does hams.com already have... a real CyberTipline reporting account" open).
# These two real base URLs are recorded here ONLY as documentation, never as a live default:
# hams_helpdesk.ncmec_api_base_url (above) starts unconfigured (see _ncmec_get_api_base_url), so
# there is no code path that reaches out to either host unless a real deployment explicitly
# configures one -- structurally the same "unset by default, warn and no-op" shape this codebase
# already uses for ham_communications_consent's own qso_recording_root.
#   Production:      https://report.cybertip.org/ispws
#   Test/sandbox:     https://exttest.cybertip.org/ispws
# The /submit request body below uses only the handful of elements NCMEC's own documentation page
# actually named (<report>/<incidentSummary>/<incidentType>, <reporter>/<reportingPerson>,
# <incidentDateTime>) -- the real XSD was not itself fetched, so this is a best-effort shape that
# MUST be validated against NCMEC's actual schema before any production submission is attempted.
# /upload, /fileinfo, /retract, and /finish are deliberately not implemented: this phase's packet
# design substitutes a playback URL for an uploaded file (see _ncmec_assemble_report_packet), and
# whether /finish is required even with zero uploaded files is unconfirmed.


class HelpdeskTicket(models.Model):
    _name = "hams_helpdesk.ticket"
    _description = "Helpdesk Ticket"
    _inherit = ["mail.thread", "mail.activity.mixin"]

    # [@ANCHOR: COMM_helpdesk_ticket_lifecycle]

    # # Verified by [@ANCHOR: COMM_test_01_ticket_creation_and_routing]
    name = fields.Char(string="Subject", required=True, tracking=True)
    description = fields.Html(string="Description")
    callsign = fields.Char(
        string="Callsign", tracking=True, help="Relevant amateur radio callsign."
    )
    ticket_type = fields.Selection(
        selection=[
            ("general", "General Inquiry"),
            # Bruce's own direct instruction, 2026-09-15: "Bugs and security intake for
            # hams_local_relay should be via hams_helpdesk." hams_local_relay is the
            # local-relay daemon (daemons/hams_local_relay in hams_com) -- this category
            # exists so the guide/UI at ham_shack/data/local_relay_guide.html has a real,
            # concrete place to send a ham reporting a bug or a security issue, rather
            # than the unfilled placeholder that prove-my-language's own audit found
            # rendering verbatim to real users. One category covers both bug reports and
            # security issues deliberately (per Bruce's own singular "a category," not
            # two) -- the description prompt on the portal form (see portal_templates.xml)
            # tells the reporter to say which kind it is; splitting this into two selection
            # values isn't needed for that.
            ("hams_local_relay", "hams_local_relay: Bug / Security Report"),
            # docs/proposals/CHILD_SAFETY_COMMUNICATIONS_CONSENT.md, section G / Phase 8. A real
            # value in the BASE selection (not a satellite module's selection_add), per that same
            # proposal's own instruction: "The actual NCMEC-reporting workflow lives in
            # hams_helpdesk itself... this piece is Open Source" -- unlike ham_repeater_dir's
            # repeater_ownership value (a hams_com-only concern bolted on from outside), this
            # workflow IS a core hams_helpdesk feature, not an extension of one.
            (_CSAM_TICKET_TYPE, "Child Safety: Apparent CSAM / Enticement / Trafficking (NCMEC Mandatory Report)"),
        ],
        string="Ticket Type",
        default="general",
        tracking=True,
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

    # # Verified by [@ANCHOR: COMM_test_06_multi_website_awareness_logic]
    website_id = fields.Many2one(
        "website",
        string="Website",
        ondelete="restrict",
        help="The website this ticket was created on.",
    )  # [@ANCHOR: COMM_helpdesk_multi_website]

    company_id = fields.Many2one(
        "res.company",
        string="Company",
        required=True,
        default=lambda self: self.env.company,
    )

    # ------------------------------------------------------------------
    # NCMEC mandatory-reporting workflow (docs/proposals/
    # CHILD_SAFETY_COMMUNICATIONS_CONSENT.md, section G / Phase 8). Populated only for
    # ticket_type == _CSAM_TICKET_TYPE -- see _ncmec_report_ticket_for_recording (the intended
    # creation entrypoint), _ncmec_assemble_report_packet, and _ncmec_apply_recording_legal_hold_
    # best_effort below for what fills each of these in.
    # ------------------------------------------------------------------
    ncmec_recording_uuid = fields.Char(
        string="QSO Recording UUID",
        index=True,
        help="ham.simulated.band.qso.recording's own recording_uuid (ham_communications_consent, "
        "hams_com) -- a loose Char reference, not a Many2one, since that model may not be "
        "installed in every deployment of this module (see _QSO_RECORDING_MODEL's own comment). "
        "Also this ticket's own dedup key: _ncmec_report_ticket_for_recording attaches a second "
        "flag for the same recording to the existing ticket rather than opening a duplicate one.",
    )
    ncmec_recording_playback_url = fields.Char(
        string="Recording Playback URL",
        help="The public /simulated-band/recordings/<uuid> playback page (qso_recording_portal.py, "
        "ham_communications_consent) -- anyone, including an NCMEC reviewer with no hams.com "
        "account, can open it directly under the proposal's own sunshine policy. Built once, at "
        "packet-assembly time, from web.base.url -- not recomputed later, so a subsequent change "
        "to web.base.url can't retroactively alter what a filed report pointed at.",
    )
    ncmec_reported_user_id = fields.Many2one(
        "res.users",
        string="Reported User",
        ondelete="set null",
        help="The hams.com account apparently responsible for the flagged interaction, when "
        "known. Left empty is a normal, expected outcome for a human QSO leg -- see comm_consent_"
        "qso_recording.py's own operator_user_id field help text for why (the SFU's own /ws "
        "endpoint does not authenticate humans today).",
    )
    ncmec_legal_hold_applied = fields.Boolean(
        string="Legal Hold Applied",
        default=False,
        help="True once this ticket has successfully called ham.simulated.band.qso.recording."
        "action_apply_legal_hold() on the linked recording (18 U.S.C. Sec. 2258A's one-year "
        "preservation duty, overriding the ordinary 30-day purge). See ncmec_legal_hold_note "
        "when this is False after creation -- it explains why, and whether an admin needs to "
        "apply the hold manually (action_ncmec_apply_legal_hold_manually below).",
    )
    ncmec_legal_hold_note = fields.Char(
        string="Legal Hold Status",
        help="Set whenever the automatic legal-hold attempt at ticket-creation time did not "
        "succeed (no matching recording, ham_communications_consent not installed, or an "
        "AccessError -- see _ncmec_apply_recording_legal_hold_best_effort's own docstring for "
        "why an AccessError is the realistic, expected outcome today). Cleared on success.",
    )
    ncmec_contact_email = fields.Char(
        string="Reporting Contact (Email)",
        help="hams.com's own designated NCMEC reporting contact, resolved from "
        "hams_helpdesk.ncmec_contact_email at packet-assembly time and stored here (not re-read "
        "live) so a later config change can't retroactively rewrite what a filed report said.",
    )
    ncmec_contact_phone = fields.Char(string="Reporting Contact (Phone)")
    ncmec_report_state = fields.Selection(
        [
            ("not_reported", "Not Reported"),
            ("report_submitted", "Report Submitted"),
            ("report_confirmed", "Report Confirmed"),
        ],
        string="NCMEC Report Status",
        default="not_reported",
        required=True,
        tracking=True,
        help="Prevents double-filing the same incident (section G's own requirement): once this "
        "is report_submitted or report_confirmed, action_ncmec_report refuses to submit again.",
    )
    ncmec_report_reference = fields.Char(
        string="NCMEC Report Reference",
        help="The report id NCMEC's own /submit response returns (real API path), or typed in "
        "by an admin after manually pasting the packet into NCMEC's own portal (fallback path).",
    )
    ncmec_report_submitted_at = fields.Datetime(string="Reported At")
    ncmec_report_submitted_by_id = fields.Many2one(
        "res.users", string="Reported By", ondelete="set null"
    )
    ncmec_report_packet = fields.Text(
        string="NCMEC Report Packet",
        help="The full assembled report packet (reported user info, recording playback URL, "
        "hams.com's own reporting-contact info) -- built once at ticket-creation time and shown "
        "read-only on the form so an admin can copy it into NCMEC's own portal if no API "
        "credentials are configured (see action_ncmec_report).",
    )

    # [@ANCHOR: hams_helpdesk:COMM_onchange_partner_id]
    @api.onchange("partner_id")
    def _onchange_partner_id(self):
        if self.partner_id and self.partner_id.callsign:
            self.callsign = self.partner_id.callsign

    @api.model_create_multi
    def create(self, vals_list):
        # [@ANCHOR: COMM_helpdesk_ticket_creation]

        # # Verified by [@ANCHOR: COMM_test_01_ticket_creation_and_routing]
        
        # Calculate on_duty_user_id before bulk creation
        on_duty_user_id = False
        Calendar = self.env["calendar.event"]
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
            # [@ANCHOR: hams_helpdesk:COMM_ncmec_force_priority]
            # "distinct, faster-SLA... routing" (section G / Phase 8): forced unconditionally,
            # not just defaulted, so a caller can't under-prioritize a mandatory-report ticket
            # by simply omitting or supplying its own priority value.
            if vals.get("ticket_type") == _CSAM_TICKET_TYPE:
                vals["priority"] = "3"

        tickets = super().create(vals_list)

        # Execute automated routing and notifications using service accounts to ensure Zero-Sudo compliance
        tickets._automated_routing_and_notification()

        # [@ANCHOR: hams_helpdesk:COMM_ncmec_packet_and_legal_hold]
        # Verified by [@ANCHOR: test_csam_ticket_creation_assembles_packet_and_forces_priority]
        # Per-ticket, not per-vals: _CSAM_TICKET_TYPE tickets need their packet assembled and the
        # linked recording's legal hold attempted the moment they exist as real rows (their own
        # ids, e.g., are part of the packet text) -- both read-only of self, so doing this after
        # super().create() returns cannot change tickets' own length/order/contents contract.
        ncmec_tickets = tickets.filtered(lambda t: t.ticket_type == _CSAM_TICKET_TYPE)
        for ticket in ncmec_tickets:
            ticket._ncmec_assemble_report_packet()
            ticket._ncmec_apply_recording_legal_hold_best_effort()

        return tickets

    # [@ANCHOR: hams_helpdesk:COMM_automated_routing_and_notification]
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
        try:
            upcoming_shifts = Calendar.get_upcoming_duty_shifts()
            upcoming_partner_ids = upcoming_shifts.mapped("user_id.partner_id.id")
        except Exception as e: # audit-ignore-catch-all
            _logger.info("Failed to get upcoming duty shifts: %s", e)

        # 2. Apply assignments and send notifications via Helpdesk Service Account
        # We explicitly do NOT catch all exceptions here to ensure that if the service
        # account is misconfigured, we fail fast.
        hd_env = utils._get_service_env("hams_helpdesk.user_helpdesk_service")

        for ticket in self:
            # Switch to service account and ensure company context is correct for each ticket
            ticket_service = ticket.with_env(hd_env).with_company(ticket.company_id)
            
            # Since assignment is now handled in create(), we only check if it was assigned
            facility_env = utils._get_service_env("zero_sudo.odoo_facility_service_internal")
            if ticket_service.user_id:
                # Email Notification
                # Bug-hunt fix (2026-09-09): this internal-only assignment
                # notice had no subtype_xmlid, so it defaulted to the
                # customer-visible mail.mt_comment subtype -- the same class
                # of leak fixed in shift_handoff.py's action_confirm_handoff
                # the same day -- and showed up in the customer's own portal
                # ticket thread even though its only intended audience is
                # the newly-assigned agent. The sibling "Shift CC" message
                # a few lines below already got this right
                # (subtype_xmlid="mail.mt_note"); this one didn't.
                ticket_service.message_post(
                    body=_("Helpdesk Ticket #%s assigned to you.") % ticket_service.id,
                    partner_ids=[ticket_service.user_id.partner_id.id],
                    subject=_("Ticket Assigned: %s") % ticket_service.name,
                    subtype_xmlid="mail.mt_note",
                )
                # Bus Toast
                facility_env["bus.bus"]._sendone(
                    ticket_service.user_id.partner_id,
                    "simple_notification",
                    {
                        "type": "warning",
                        "title": _("New Helpdesk Ticket"),
                        "message": _("Ticket %s requires your attention.")
                        % ticket_service.name,
                    },
                )

            # [@ANCHOR: hams_helpdesk:COMM_ncmec_notify_all_managers]
            # Verified by [@ANCHOR: test_csam_ticket_creation_notifies_every_helpdesk_manager]
            # "Faster-SLA... routing", the other half (see _CSAM_TICKET_TYPE's own comment):
            # a mandatory federal report can't wait out a gap in on-duty coverage the way an
            # ordinary ticket reasonably can, so every group_helpdesk_manager member is notified
            # immediately and unconditionally here -- on top of, never instead of, the ordinary
            # on-duty assignment above. Reuses the exact same message_post/bus.bus._sendone
            # notification machinery already used two blocks up ("reusing its existing on-duty
            # routing machinery, not a new escalation system" -- section G's own words).
            if ticket_service.ticket_type == _CSAM_TICKET_TYPE:
                # Resolved and read via hd_env (the service account), not self.env (whatever
                # env actually called create() -- could be a low-privilege portal-ish caller in
                # the general case): res.groups.user_ids -> res.users -> res.partner reads must
                # not depend on the CALLER happening to already have read access to those models.
                manager_partners = (
                    hd_env.ref("hams_helpdesk.group_helpdesk_manager")
                    .user_ids.mapped("partner_id")
                )
                if manager_partners:
                    ticket_service.message_subscribe(
                        partner_ids=manager_partners.ids
                    )
                    ticket_service.message_post(
                        body=_(
                            "URGENT: apparent CSAM / online enticement / child sex trafficking "
                            "flagged. hams.com has an independent federal legal duty (18 U.S.C. "
                            "Sec. 2258A) to report this to NCMEC's CyberTipline as soon as "
                            "reasonably possible. Review immediately."
                        ),
                        partner_ids=manager_partners.ids,
                        subject=_("URGENT Child Safety Report: %s") % ticket_service.name,
                        subtype_xmlid="mail.mt_note",
                    )
                    for partner in manager_partners:
                        facility_env["bus.bus"]._sendone(
                            partner,
                            "simple_notification",
                            {
                                "type": "danger",
                                "title": _("URGENT: Child Safety Report"),
                                "message": _(
                                    "Ticket %s requires immediate review (mandatory NCMEC "
                                    "report)."
                                )
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
        # [@ANCHOR: COMM_helpdesk_micro_privilege]
        # Micro-Privilege Security Audit: Prevent portal users from modifying restricted fields.
        if self.env.user.has_group("base.group_portal"):
            restricted_fields = {
                "stage",
                "user_id",
                "priority",
                "calendar_event_id",
                "website_id",
                "company_id",
                # bug-hunt (2026-09-13): partner_id was missing from this set, and carries no
                # field-level `groups=` restriction of its own either. ir.rule's write-time
                # access check validates the domain against the record's CURRENT (pre-write)
                # state, so a portal caller whose own partner_id matches the rule today could
                # write partner_id to an arbitrary OTHER partner's id -- removing the ticket
                # from their own portal view and, if that partner is also a portal user,
                # inserting it (with its full history) into a stranger's. Confirmed exploitable
                # via a real probe test before this fix, converted below into
                # test_05b_portal_cannot_reassign_partner_id.
                "partner_id",
                # NCMEC mandatory-reporting fields (section G / Phase 8): a portal user must
                # never be able to fabricate ncmec_report_state='report_confirmed' (hiding a
                # real report from an admin's own "was this actually filed" check) or clear
                # ncmec_legal_hold_applied (defeating the 2258A preservation hold).
                "ncmec_report_state",
                "ncmec_legal_hold_applied",
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
        # [@ANCHOR: COMM_helpdesk_shift_handoff]

        # # Verified by [@ANCHOR: COMM_test_02_shift_handoff_wizard]
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
        # [@ANCHOR: COMM_helpdesk_portal_close]
        self.ensure_one()
        # Security: Ensure the user is the owner of the ticket
        if self.partner_id != self.env.user.partner_id and not self.env.user.has_group(
            "hams_helpdesk.group_helpdesk_user"
        ):
            raise AccessError(_("You are not authorized to close this ticket."))

        if self.stage != "closed":
            utils = self.env["zero_sudo.security.utils"]
            hd_env = utils._get_service_env("hams_helpdesk.user_helpdesk_service")
            self.with_env(hd_env).with_context(mail_notrack=True).write({"stage": "closed"})
            self.with_env(hd_env).message_post(body=_("Ticket closed by customer."))

    def message_new(self, msg_dict, custom_values=None):
        """Overrides mail.thread's own default so a ticket created from an
        inbound email (see ingest_inbound_email() below) actually gets
        linked to the sending customer, the same way a ticket created via
        the portal or the backend already is.

        Bug-hunt fix (2026-09-09): mail.thread.message_new()'s own default
        implementation only auto-populates a field via
        _mail_get_primary_email_field(), and hams_helpdesk.ticket has no
        such field (no email_from/email Char) -- so partner_id was always
        False on an email-ingested ticket, even when message_route() had
        already resolved the sender's address to an existing partner
        (msg_dict['author_id']). With partner_id unset: the customer's own
        /my/tickets never lists the ticket (portal.py's own domain filters
        on partner_id = the logged-in user's partner), write()'s
        stage-change mail-back never fires for it (`if ticket.partner_id:
        ...`), and _automated_routing_and_notification()'s own
        "ensure customer is subscribed" step silently does nothing. Setting
        partner_id here also makes create()'s own existing
        callsign-from-partner fallback apply for free.
        """
        # [@ANCHOR: COMM_helpdesk_message_new]
        values = dict(custom_values or {})
        author_id = msg_dict.get("author_id")
        if author_id and not values.get("partner_id"):
            values["partner_id"] = author_id
        return super().message_new(msg_dict, custom_values=values)

    def ingest_inbound_email(self, raw_email_bytes):
        """RPC entrypoint for the SES-to-S3-to-Odoo inbound mail daemon
        (see docs/proposals/EMAIL_SEND_RECEIVE.md). Restricted to the
        dedicated mail-ingest service account: a custom RPC method isn't
        auto-gated by ir.model.access the way create/write/unlink are, so
        this check is the real access boundary, not the CSV entry.

        ``raw_email_bytes`` arrives as a base64 string, not raw bytes --
        the daemon calls this over the JSON-2 API, whose transport is
        text, and a raw MIME message is not guaranteed to be valid UTF-8
        (attachments, non-ASCII bodies without a text-safe
        Content-Transfer-Encoding).
        """
        # [@ANCHOR: COMM_helpdesk_mail_ingest]
        if self.env.user.login != "mail_ingest_service_internal":
            raise AccessError(
                _("Only the mail-ingest service account may call this method.")
            )
        message_bytes = base64.b64decode(raw_email_bytes)
        self.env["mail.thread"].sudo().with_company(self.env.company).message_process(None, message_bytes)  # burn-ignore-sudo: message_process() unconditionally sudo()s internally on any alias match (see mail_thread.py); identical, already user-verified reasoning as ses_webhook/controllers/webhook_api.py's matched-sender call.

    # ------------------------------------------------------------------
    # NCMEC mandatory-reporting workflow (docs/proposals/
    # CHILD_SAFETY_COMMUNICATIONS_CONSENT.md, section G / Phase 8)
    # ------------------------------------------------------------------

    # [@ANCHOR: hams_helpdesk:COMM_ncmec_report_ticket_for_recording]
    # Verified by [@ANCHOR: test_report_ticket_for_recording_creates_a_new_ticket]
    # Verified by [@ANCHOR: test_report_ticket_for_recording_attaches_to_an_existing_ticket]
    @api.model
    def _ncmec_report_ticket_for_recording(self, recording_uuid, vals):
        """The intended creation entrypoint for a flagged CSAM/enticement/trafficking
        interaction -- what a future caller (Phase 7's bot self-reporting / Official Observer,
        docs/proposals/CHILD_SAFETY_COMMUNICATIONS_CONSENT.md section G, developed privately in
        hams_com, not part of this phase) is expected to call, the same "one gated, documented
        entrypoint, not left to create() + ir.model.access alone" shape comm_consent_qso_
        recording.py's own _record_ingest already establishes for this exact proposal.

        Idempotent by recording, not by a hard uniqueness constraint on the column: "before
        creating a new... ticket for a given recording, check whether an existing ticket already
        covers that same recording... and attach to the existing ticket... rather than opening a
        duplicate one" (section G, verbatim) -- bot self-reporting, the Official Observer, and an
        ordinary member/public report could all independently flag the same QSO. A *closed*
        prior ticket does NOT count as still covering the recording (see the domain below): if a
        first flag was somehow resolved as a false positive, a genuinely new later flag on the
        same recording must still open a fresh ticket, not silently attach to a dead one nobody
        is looking at.

        ``vals`` is the same create()-shaped dict a caller would otherwise pass to create()
        directly (name, description, callsign, partner_id, ...) -- this method fills in
        ticket_type and priority itself (matching create()'s own COMM_ncmec_force_priority
        behavior) and must not be passed either key.
        """
        if "ticket_type" in vals or "priority" in vals:
            raise UserError(
                _(
                    "_ncmec_report_ticket_for_recording sets ticket_type and priority itself; "
                    "do not pass either in vals."
                )
            )
        existing = self.search(
            [
                ("ticket_type", "=", _CSAM_TICKET_TYPE),
                ("ncmec_recording_uuid", "=", recording_uuid),
                ("stage", "not in", ["resolved", "closed"]),
            ],
            limit=1,
        )
        if existing:
            existing.message_post(
                body=_(
                    "Another flag was received for this same recording (uuid %s) -- attached "
                    "here rather than opening a duplicate ticket."
                )
                % recording_uuid,
                subtype_xmlid="mail.mt_note",
            )
            return existing

        create_vals = dict(vals, ticket_type=_CSAM_TICKET_TYPE)
        create_vals["ncmec_recording_uuid"] = recording_uuid
        return self.create([create_vals])

    # [@ANCHOR: hams_helpdesk:COMM_ncmec_assemble_report_packet]
    # Verified by [@ANCHOR: test_assemble_report_packet_builds_playback_url_and_contact_info]
    def _ncmec_assemble_report_packet(self):
        """Builds the whole report packet as structured fields on the ticket itself, per
        section G: "An admin reviewing the ticket should see a fully assembled report, not a
        bare flag they have to go build a case file around themselves." Called once, right
        after creation (see create()'s own COMM_ncmec_packet_and_legal_hold) -- every value
        written here is a snapshot at that moment, deliberately never recomputed later (a
        `Text`/`Char` field set directly, not an `@api.depends` compute), so a later config or
        data change can't retroactively rewrite what an already-filed report actually said."""
        self.ensure_one()
        utils = self.env["zero_sudo.security.utils"]
        contact_email = utils._get_system_param(_NCMEC_PARAM_CONTACT_EMAIL, "admin@hams.com")
        contact_phone = utils._get_system_param(_NCMEC_PARAM_CONTACT_PHONE, "+1 510-473-7367")

        playback_url = False
        if self.ncmec_recording_uuid:
            base_url = utils._get_system_param("web.base.url", "") or ""
            playback_url = (
                f"{base_url.rstrip('/')}/simulated-band/recordings/{self.ncmec_recording_uuid}"
            )

        reported_user = self.ncmec_reported_user_id
        if not reported_user and self.partner_id:
            # A reasonable default when the caller didn't already resolve a specific
            # ncmec_reported_user_id: the ticket's own partner_id (e.g. bot self-reporting,
            # which already knows the human operator's account -- see comm_consent_qso_
            # recording.py's own is_bot/operator_user_id fields). Left False, not guessed
            # further, when partner_id has no linked res.users (an honest gap, matching this
            # whole proposal's own established "False is a normal, expected outcome" precedent
            # rather than a silent wrong guess).
            reported_user = self.env["res.users"].search(
                [("partner_id", "=", self.partner_id.id)], limit=1
            )

        packet_lines = [
            _("NCMEC CyberTipline Report Packet"),
            _("Ticket: #%(id)s -- %(name)s") % {"id": self.id, "name": self.name},
            _("Category: Apparent CSAM / Online Enticement / Child Sex Trafficking "
              "(18 U.S.C. Sec. 2258A, REPORT Act)"),
            "",
            _("-- Reported User --"),
        ]
        if reported_user:
            packet_lines += [
                _("Callsign: %s") % (reported_user.partner_id.callsign or _("(none on file)")),
                _("Account created: %s") % (reported_user.create_date or _("unknown")),
                _("hams.com user id: %s") % reported_user.id,
            ]
        else:
            packet_lines.append(_("(No hams.com account could be identified for this report.)"))
        packet_lines += [
            "",
            _("-- QSO Recording --"),
            _("Recording UUID: %s") % (self.ncmec_recording_uuid or _("(none)")),
            _("Playback URL: %s") % (playback_url or _("(none)")),
            "",
            _("-- hams.com Reporting Contact --"),
            _("Email: %s") % contact_email,
            _("Phone: %s") % contact_phone,
        ]

        self.write(
            {
                "ncmec_recording_playback_url": playback_url or False,
                "ncmec_reported_user_id": reported_user.id if reported_user else False,
                "ncmec_contact_email": contact_email,
                "ncmec_contact_phone": contact_phone,
                "ncmec_report_packet": "\n".join(packet_lines),
            }
        )

    # [@ANCHOR: hams_helpdesk:COMM_ncmec_recording_model_installed]
    def _ncmec_recording_model_installed(self):
        """Whether ham_communications_consent (hams_com) happens to be installed alongside this
        module -- its own method, not an inline `_QSO_RECORDING_MODEL not in self.env` check in
        the caller below, purely so a test can patch this one boolean seam (safe_patch_object)
        to exercise the "model present" branches of _ncmec_apply_recording_legal_hold_best_
        effort without hams_com actually being installed in this hams_open-only test run."""
        return _QSO_RECORDING_MODEL in self.env  # burn-ignore-optional-cross-repo-dep: ham_communications_consent (hams_com) may not be installed -- hams_open/hams_helpdesk must remain installable and testable standalone. See _QSO_RECORDING_MODEL's own module-level comment.

    # [@ANCHOR: hams_helpdesk:COMM_ncmec_attempt_legal_hold_call]
    def _ncmec_attempt_legal_hold_call(self, model_name):
        """The real search()+action_apply_legal_hold() call, factored out to its own method
        purely so tests can patch this one seam (safe_patch_object, matching this test file's
        own established get_current_on_duty_admin precedent) to exercise the success and
        AccessError branches of _ncmec_apply_recording_legal_hold_best_effort below without
        ham_communications_consent (hams_com) actually being installed. Returns True on success,
        False if no matching recording row was found; raises AccessError exactly as the real ORM
        call would, uncaught -- the caller is responsible for catching it."""
        self.ensure_one()
        recording = self.env[model_name].search(
            [("recording_uuid", "=", self.ncmec_recording_uuid)], limit=1
        )
        if not recording:
            return False
        recording.action_apply_legal_hold(
            reason=_("NCMEC mandatory-report ticket #%s") % self.id
        )
        return True

    # [@ANCHOR: hams_helpdesk:COMM_ncmec_apply_recording_legal_hold_best_effort]
    # Verified by [@ANCHOR: test_legal_hold_best_effort_noops_when_recording_model_not_installed]
    # Verified by [@ANCHOR: test_legal_hold_best_effort_records_access_error_gracefully]
    # Verified by [@ANCHOR: test_legal_hold_best_effort_succeeds_when_call_succeeds]
    def _ncmec_apply_recording_legal_hold_best_effort(self):
        """Attempts to apply the legal hold "before, not after, a human reviewer confirms it"
        (section G, verbatim) -- in the AMBIENT env this runs under (whatever created the
        ticket), never a newly-minted service account of hams_helpdesk's own. That's a
        deliberate choice, not an oversight: ham_communications_consent's own action_apply_
        legal_hold is documented (comm_consent_qso_recording.py) as restricted to base.
        group_system "until that phase adds its own scoped grant" -- and granting that scoped
        access is a change to ham_communications_consent's own ir.model.access.csv, which lives
        in hams_com. This phase cannot make that grant (out of scope: no hams_com edits), and
        hams_open minting its OWN service-account identity here would be hollow -- hams_com has
        never been asked to empower an identity hams_open invents unilaterally, so it would only
        ever hit the exact same AccessError a moment later.

        So the realistic outcome TODAY, for a ticket created via the ordinary hams_helpdesk
        service account (group_helpdesk_manager, not base.group_system), is the graceful-
        degradation branch below: the attempt is made, it is expected to fail, and it fails
        loud (ncmec_legal_hold_note on the ticket, a WARNING log line) rather than silent. A
        real admin -- who DOES hold base.group_system in this codebase's own established
        convention (helpdesk_security.xml seeds base.user_admin/base.user_root into group_
        helpdesk_manager) -- can apply the hold by hand via action_ncmec_apply_legal_hold_
        manually below, which runs in THEIR OWN ambient env and therefore actually succeeds.
        The proper long-term fix -- a scoped ham_communications_consent service-account grant
        for whatever identity Phase 7 (hams_com) eventually creates tickets as -- is real,
        concrete follow-up work on the hams_com side, not something this hams_open-only phase
        can complete itself."""
        self.ensure_one()
        if not self.ncmec_recording_uuid:
            return
        if not self._ncmec_recording_model_installed():
            self.ncmec_legal_hold_note = _(
                "ham_communications_consent is not installed in this deployment -- apply the "
                "legal hold directly on the recording once it is."
            )
            _logger.warning(
                "NCMEC ticket #%s: could not apply legal hold, "
                "ham_communications_consent is not installed.",
                self.id,
            )
            return
        try:
            applied = self._ncmec_attempt_legal_hold_call(_QSO_RECORDING_MODEL)
        except AccessError as e:
            self.ncmec_legal_hold_note = _(
                "Automatic legal hold failed (%s) -- an administrator must apply it manually "
                "(see the Apply Legal Hold Manually button)."
            ) % e
            _logger.warning(
                "NCMEC ticket #%s: AccessError applying legal hold to recording %s: %s",
                self.id,
                self.ncmec_recording_uuid,
                e,
            )
            return
        if not applied:
            self.ncmec_legal_hold_note = _(
                "No QSO recording row found for uuid %s yet -- it may still be mid-ingest; "
                "retry once it appears."
            ) % self.ncmec_recording_uuid
            return
        self.write({"ncmec_legal_hold_applied": True, "ncmec_legal_hold_note": False})

    # [@ANCHOR: hams_helpdesk:COMM_ncmec_action_apply_legal_hold_manually]
    # Verified by [@ANCHOR: test_action_apply_legal_hold_manually_requires_group_system]
    def action_ncmec_apply_legal_hold_manually(self):
        """The real fallback path for the AccessError case _ncmec_apply_recording_legal_hold_
        best_effort's own docstring describes: gated to base.group_system, matching ham_
        repeater_dir's own action_repeater_revoke_ownership precedent for an equivalently
        sensitive admin-only action on this same ticket model."""
        self.ensure_one()
        if not (self.env.su or self.env.user.has_group("base.group_system")):
            raise AccessError(
                _("Only an administrator can apply a legal hold manually.")
            )
        if self.ticket_type != _CSAM_TICKET_TYPE:
            raise UserError(_("This ticket is not a child-safety mandatory-report ticket."))
        self._ncmec_apply_recording_legal_hold_best_effort()
        if not self.ncmec_legal_hold_applied:
            raise UserError(
                self.ncmec_legal_hold_note
                or _("Applying the legal hold failed for an unknown reason.")
            )

    # [@ANCHOR: hams_helpdesk:COMM_ncmec_action_report]
    # Verified by [@ANCHOR: test_action_ncmec_report_requires_group_system]
    # Verified by [@ANCHOR: test_action_ncmec_report_refuses_a_second_submission]
    # Verified by [@ANCHOR: test_action_ncmec_report_without_credentials_explains_manual_fallback]
    # Verified by [@ANCHOR: test_action_ncmec_report_submits_via_the_real_api_when_configured]
    def action_ncmec_report(self):
        """The admin-facing "Report" button (section G). Gated to base.group_system for the
        same reason action_ncmec_apply_legal_hold_manually is: filing a federal mandatory report
        is not an ordinary helpdesk-manager action. Refuses outright (server-side, not just a
        disabled/hidden button -- section G's own explicit "inert, not just hidden" requirement)
        once ncmec_report_state is no longer 'not_reported', so this can never double-file the
        same incident."""
        self.ensure_one()
        if not (self.env.su or self.env.user.has_group("base.group_system")):
            raise AccessError(_("Only an administrator can file an NCMEC report."))
        if self.ticket_type != _CSAM_TICKET_TYPE:
            raise UserError(_("This ticket is not a child-safety mandatory-report ticket."))
        if self.ncmec_report_state != "not_reported":
            raise UserError(
                _("This incident was already reported to NCMEC (status: %s).")
                % dict(self._fields["ncmec_report_state"]._description_selection(self.env)).get(
                    self.ncmec_report_state
                )
            )

        utils = self.env["zero_sudo.security.utils"]
        base_url = utils._get_system_param(_NCMEC_PARAM_BASE_URL, "")
        if not base_url:
            # No real API credentials configured -- see this module's own top-of-file comment
            # on NCMEC's real CyberTipline Reporting API for why this is the honest default
            # today (credentials must be requested from NCMEC directly; hams.com does not have
            # them yet). The packet is already assembled and visible on the form
            # (ncmec_report_packet) -- an admin copies it into NCMEC's own reporting portal,
            # then calls action_ncmec_mark_report_filed_manually below once that's done.
            raise UserError(
                _(
                    "No NCMEC API credentials are configured (hams_helpdesk.ncmec_api_base_url "
                    "is unset). Copy the report packet below into NCMEC's own CyberTipline "
                    "reporting portal (https://report.cybertip.org) and then use 'Mark Filed "
                    "Manually' to record that this incident has been reported."
                )
            )
        self._ncmec_submit_report_via_api(base_url)

    # [@ANCHOR: hams_helpdesk:COMM_ncmec_submit_report_via_api]
    def _ncmec_submit_report_via_api(self, base_url):
        """Real HTTP call to NCMEC's own CyberTipline Reporting API -- see this module's own
        top-of-file comment for the source of every claim below (base URLs, HTTP Basic Auth,
        the element names used). UNTESTED against NCMEC's real API: no real (even sandbox)
        credentials exist for this session to test against, matching this codebase's own
        established "raw REST call, mocked at the boundary in tests, must be re-verified the
        moment real credentials exist" precedent (ham_club_management/models/res_partner.py's
        own Stripe integration carries the identical caveat, verbatim).

        Uses zero_sudo.daemon.ssrf_safe_fetch.urlopen_ssrf_safe, not a bare urlopen()/requests
        call -- base_url is an admin-configured ir.config_parameter value, not a hardcoded
        literal, so it is genuinely NOT the "fixed and trusted host" case check_burn_list.py's
        own OUTBOUND FETCH rule carves out for audit-ignore-outbound-fetch; a misconfigured or
        compromised config value pointing at an internal address is exactly the DNS-rebinding /
        SSRF risk that helper exists to close (see binary_downloader/models/binary_utils.py's
        own established call-site pattern, mirrored here)."""
        self.ensure_one()
        utils = self.env["zero_sudo.security.utils"]
        username = utils._get_system_param(_NCMEC_PARAM_USERNAME, "")
        password = utils._get_system_param(_NCMEC_PARAM_PASSWORD, "")
        if not username or not password:
            raise UserError(
                _(
                    "hams_helpdesk.ncmec_api_base_url is configured but the username/password "
                    "are not -- both are required for HTTP Basic Auth against NCMEC's API."
                )
            )

        report = ET.Element("report")
        incident_summary = ET.SubElement(report, "incidentSummary")
        ET.SubElement(incident_summary, "incidentType").text = (
            "Online Enticement of a Child for Sexual Acts, or Child Sex Trafficking, or "
            "Child Pornography (possession, manufacture, and distribution)"
        )
        ET.SubElement(incident_summary, "incidentDateTime").text = fields.Datetime.to_string(
            fields.Datetime.now()
        )
        reporter = ET.SubElement(report, "reporter")
        reporting_person = ET.SubElement(reporter, "reportingPerson")
        ET.SubElement(reporting_person, "email").text = self.ncmec_contact_email or ""
        payload = ET.tostring(report, encoding="utf-8")

        basic_auth = base64.b64encode(f"{username}:{password}".encode("utf-8")).decode("ascii")
        request = urllib.request.Request(
            f"{base_url.rstrip('/')}/submit",
            data=payload,
            method="POST",
            headers={
                "Content-Type": "application/xml",
                "Authorization": f"Basic {basic_auth}",
            },
        )

        try:
            with _urlopen_ssrf_safe(
                request, "hams_helpdesk_ncmec_report", https_only=True, timeout=30
            ) as response:
                response_body = response.read().decode("utf-8", errors="replace")
        except urllib.error.HTTPError as e:
            try:
                response_body = e.read().decode("utf-8", errors="replace")
            except (OSError, AttributeError):
                response_body = ""
            _logger.warning(
                "NCMEC ticket #%s: /submit returned HTTP %s: %s",
                self.id,
                e.code,
                response_body[:500],
            )
            raise UserError(
                _("NCMEC's reporting API rejected the submission (HTTP %s). See the server "
                  "log for the full response.") % e.code
            ) from e
        except (urllib.error.URLError, SSRFValidationError) as e:
            _logger.warning("NCMEC ticket #%s: /submit request failed: %s", self.id, e)
            raise UserError(
                _("Could not reach NCMEC's reporting API: %s") % e
            ) from e

        report_id = False
        try:
            response_root = ET.fromstring(response_body)
            report_id_el = response_root.find(".//reportId")
            if report_id_el is not None:
                report_id = report_id_el.text
        except ET.ParseError as e:
            _logger.warning(
                "NCMEC ticket #%s: could not parse /submit response as XML: %s", self.id, e
            )

        self.write(
            {
                "ncmec_report_state": "report_submitted",
                "ncmec_report_reference": report_id or False,
                "ncmec_report_submitted_at": fields.Datetime.now(),
                "ncmec_report_submitted_by_id": self.env.user.id,
            }
        )

    # [@ANCHOR: hams_helpdesk:COMM_ncmec_action_mark_report_filed_manually]
    # Verified by [@ANCHOR: test_action_mark_report_filed_manually_requires_group_system]
    # Verified by [@ANCHOR: test_action_mark_report_filed_manually_refuses_a_second_submission]
    def action_ncmec_mark_report_filed_manually(self):
        """The fallback half of the "Report" button flow (section G): after an admin has
        copied ncmec_report_packet into NCMEC's own reporting portal by hand (no API
        credentials configured -- see action_ncmec_report), this records that the incident has
        been reported, with the same double-file guard action_ncmec_report itself uses. The
        real NCMEC-issued reference (if the portal gives one) is typed into ncmec_report_
        reference directly on the form afterward -- an ordinary field edit, not a separate
        wizard, matching this ticket model's own existing preference for plain field edits over
        wizards where a value has no other structure to validate."""
        self.ensure_one()
        if not (self.env.su or self.env.user.has_group("base.group_system")):
            raise AccessError(_("Only an administrator can mark an NCMEC report as filed."))
        if self.ticket_type != _CSAM_TICKET_TYPE:
            raise UserError(_("This ticket is not a child-safety mandatory-report ticket."))
        if self.ncmec_report_state != "not_reported":
            raise UserError(
                _("This incident was already reported to NCMEC (status: %s).")
                % dict(self._fields["ncmec_report_state"]._description_selection(self.env)).get(
                    self.ncmec_report_state
                )
            )
        self.write(
            {
                "ncmec_report_state": "report_submitted",
                "ncmec_report_submitted_at": fields.Datetime.now(),
                "ncmec_report_submitted_by_id": self.env.user.id,
            }
        )
