# This software is distributed under the terms of the Affero General Public License (AGPL-3).
# SPDX-License-Identifier: AGPL-3.0-or-later

# -*- coding: utf-8 -*-
import datetime
import logging
from odoo import models, fields, api, _
from odoo.addons.distributed_redis_cache.redis_pool import redis, redis_pool

_logger = logging.getLogger(__name__)


class PagerIncident(models.Model):
    """
    Represents an incident detected by the monitoring system.
    This model is multi-tenant and multi-website, partitioned by website_id.
    """

    _name = "pager.incident"
    _description = "Pager Duty Incident"
    _inherit = ["mail.thread"]

    name = fields.Char(
        string="Incident ID", required=True, copy=False, readonly=True, default="New"
    )
    website_id = fields.Many2one(
        "website", string="Website", ondelete="cascade", index=True
    )
    company_id = fields.Many2one(
        "res.company", string="Company", required=True, default=lambda self: self.env.company
    )
    # Added index=True to prevent sequential scans during daemon polling
    source = fields.Char(string="Source", required=True, index=True, tracking=True)
    severity = fields.Selection(
        [
            ("low", "Low"),
            ("medium", "Medium"),
            ("high", "High"),
            ("critical", "Critical"),
        ],
        string="Severity",
        required=True,
        tracking=True,
    )
    description = fields.Text(string="Description", required=True, tracking=True)
    # Added index=True to prevent sequential scans during daemon polling
    status = fields.Selection(
        [("open", "Open"), ("acknowledged", "Acknowledged"), ("resolved", "Resolved")],
        string="Status",
        default="open",
        index=True,
        tracking=True,
    )
    # Added index=True to prevent sequential scans during cron escalations
    is_escalated = fields.Boolean(string="Escalated", default=False, index=True)
    time_acknowledged = fields.Datetime(string="Acknowledged At", readonly=True)
    time_resolved = fields.Datetime(string="Resolved At", readonly=True)
    acknowledged_by_id = fields.Many2one(
        "res.users", string="Acknowledged By", readonly=True, tracking=True
    )
    mtta = fields.Float(
        string="MTTA (Minutes)", compute="_compute_mtta", store=True, help="Mean Time To Acknowledge"
    )
    mttr = fields.Float(
        string="MTTR (Minutes)", compute="_compute_mttr", store=True, help="Mean Time To Resolve"
    )

    @api.depends("time_acknowledged", "create_date")
    def _compute_mtta(self):
        for rec in self:
            if rec.time_acknowledged and rec.create_date:
                rec.mtta = (rec.time_acknowledged - rec.create_date).total_seconds() / 60.0
            else:
                rec.mtta = 0.0

    @api.depends("time_resolved", "create_date")
    def _compute_mttr(self):
        for rec in self:
            if rec.time_resolved and rec.create_date:
                rec.mttr = (rec.time_resolved - rec.create_date).total_seconds() / 60.0
            else:
                rec.mttr = 0.0
    helpdesk_ticket_id = fields.Integer(
        string="Helpdesk Ticket ID",
        help="Stores the integer ID of the generated helpdesk ticket to remain schema-agnostic.",
        tracking=True,
    )
    helpdesk_ticket_model = fields.Char(
        string="Ticket Model",
        help="The Odoo model used for the ticket (e.g. hams_helpdesk.ticket or helpdesk.ticket).",
    )
    # [@ANCHOR: pager_incident_occurrence_count]
    # Added 2026-09-01 per ODOO_DB_LOG_CRASH_MONITORING.md's own "What's
    # actually missing" #1: report_incident()'s dedup branch (see its own
    # report_incident_rate_limit anchor comment) already finds an existing
    # open/acknowledged incident for the same source and returns its id
    # without creating a duplicate -- but nothing tracked how many times
    # it had recurred. default=1 so every already-existing incident
    # (before this field existed) reads as "happened once," the correct
    # backward-compatible interpretation.
    occurrence_count = fields.Integer(
        string="Occurrences",
        default=1,
        tracking=True,
        help="How many times this same source has reported while an incident for it was still open/acknowledged.",
    )
    last_occurred = fields.Datetime(
        string="Last Occurred",
        default=lambda self: fields.Datetime.now(),
        help="When this source's own report_incident() call was last received, including repeats that did not create a new incident.",
    )

    def write(self, vals):
        now = fields.Datetime.now()
        if vals.get("status") == "acknowledged":
            vals["time_acknowledged"] = now
            if not vals.get("acknowledged_by_id"):
                vals["acknowledged_by_id"] = self.env.user.id
        elif vals.get("status") == "resolved":
            vals["time_resolved"] = now

        res = super(PagerIncident, self.with_context(mail_notrack=True)).write(vals)

        # MTTA and MTTR are now handled via computed fields.

        if self:
            self.env["bus.bus"]._sendone("pager_duty", "update_board", {})

        return res

    @api.model
    def action_escalate_unacknowledged(self):
        """
        Escalates unacknowledged incidents older than 15 minutes.
        Groups escalations by website to maintain multi-tenant isolation in notifications.
        """
        # [@ANCHOR: test_pager_escalation]
        fifteen_mins_ago = fields.Datetime.now() - datetime.timedelta(minutes=15)
        # Security: search() on Pager Duty records must be performed by a service account
        # to ensure minimum privilege. We use the pager service internal user to execute
        # search and write, and the mail service to execute message_post.
        pd_svc = self.env["zero_sudo.security.utils"]._get_service_uid(
            "pager_duty.user_pager_service_internal"
        )
        mail_svc = self.env["zero_sudo.security.utils"]._get_service_uid(
            "zero_sudo.mail_service_internal"
        )
        IncidentModel = self.env["pager.incident"].with_user(pd_svc)

        incidents = IncidentModel.search(
            [
                ("status", "=", "open"),
                ("is_escalated", "=", False),
                ("create_date", "<", fifteen_mins_ago),
            ],
            limit=1000,
        )
        if not incidents:
            return

        pager_admin_group = self.env.ref("pager_duty.group_pager_admin")

        # Group incidents by website_id to ensure relevant admins are notified
        # if the deployment has per-website admin groups (future expansion).
        # For now, we respect website isolation in the message posting.
        for inc in incidents:
            partners = pager_admin_group.user_ids.filtered(
                lambda u: not inc.website_id
                or ("website_id" in u._fields and u.website_id == inc.website_id)
            ).mapped("partner_id")

            if not partners:
                partners = pager_admin_group.user_ids.mapped("partner_id")

            msg_body = _("🚨 ESCALATION: Incident open for > 15 minutes!")
            inc.with_user(mail_svc).message_post(
                body=msg_body, partner_ids=partners.ids)   # fmt: skip
        incidents.write({"is_escalated": True})

    @api.model
    def report_incident(self, vals):
        """
        Reports a new incident. Supports multi-website partitioning.
        """
        # [@ANCHOR: report_incident_rate_limit]
        source = vals.get("source", "unknown")
        website_id = vals.get("website_id") or self.env.context.get("website_id")

        # Strict schema enforcement, no generic error masking
        if not website_id:
            current_website = self.env["website"].get_current_website()
            if current_website:
                website_id = current_website.id

        # [@ANCHOR: pd_redis_rate_limit]
        redis_key = f"pager_rate_limit:{source}:{website_id or 'global'}"

        if redis and redis_pool:
            try:
                r_client = redis.Redis(connection_pool=redis_pool)
                # SET with NX=True and EX=60 provides an atomic rate limit check-and-set
                if not r_client.set(redis_key, "1", ex=60, nx=True):
                    return False
            except (
                redis.exceptions.RedisError,
                Exception,
            ) as e:  # audit-ignore-catch-all
                _logger.warning("Redis rate limit check failed: %s", e)

        svc_uid = self.env["zero_sudo.security.utils"]._get_service_uid(
            "pager_duty.user_pager_incident_creator"
        )
        IncidentModel = self.env["pager.incident"].with_user(svc_uid)

        search_domain = [
            ("source", "=", vals.get("source", "unknown")),
            ("status", "in", ["open", "acknowledged"]),
        ]
        if website_id:
            search_domain.append(("website_id", "=", website_id))
            vals["website_id"] = website_id

        existing = IncidentModel.search(search_domain, limit=1)
        if existing:
            existing.write(
                {
                    "occurrence_count": existing.occurrence_count + 1,
                    "last_occurred": fields.Datetime.now(),
                }
            )
            return existing.id

        if vals.get("name", "New") == "New":
            vals["name"] = "INC-AUTO"

        incident = IncidentModel.create(vals)
        on_duty_user = (
            self.env["calendar.event"]
            .with_context(website_id=website_id)
            .get_current_on_duty_admin()
        )

        # Suppress native pager notifications if helpdesk integration is active
        # to prevent duplicate alerting (Helpdesk will handle the page).
        use_helpdesk = self.env["zero_sudo.security.utils"]._get_system_param(
            "pager_duty.helpdesk_model"
        )

        if on_duty_user and not use_helpdesk:
            mail_svc = self.env["zero_sudo.security.utils"]._get_service_uid(
                "zero_sudo.mail_service_internal"
            )
            msg_body = _("New Incident Created")
            partner_ids = [on_duty_user.partner_id.id]
            incident.with_user(mail_svc).message_post(
                body=msg_body, partner_ids=partner_ids)   # fmt: skip
        return incident.id

    def message_new(self, msg_dict, custom_values=None):
        """Called by Odoo's own mailgateway (message_process -> message_route
        -> here, via a mail.alias pointing at this model -- see
        data/mail_alias_data.xml and hooks.py's info@ claim) when an inbound
        email doesn't match an existing thread. Used for info@hams.com and
        postmaster@hams.com routing (docs/proposals/EMAIL_SEND_RECEIVE.md).

        Deliberately does NOT call report_incident(): that method's Redis
        rate-limit and same-source dedup (see [@ANCHOR: pd_redis_rate_limit])
        exist to collapse repeated automated signals reporting the same
        underlying problem (a monitor re-detecting the same outage every
        poll), not to gate genuinely distinct human email inquiries -- a
        constant "source" there would silently drop or merge unrelated
        emails from different people. Odoo's own mailgateway already
        threads replies to an existing incident natively (message_update(),
        matched by Message-ID/References), so no separate dedup is needed
        for this path.
        """
        # [@ANCHOR: pager_incident_message_new]
        data = dict(custom_values or {})
        sender = msg_dict.get("email_from") or "unknown sender"
        source_prefix = data.pop("incident_source_prefix", "email")
        data.setdefault("name", msg_dict.get("subject") or f"Email from {sender}")
        # Per-sender, not a constant -- see docstring above.
        data["source"] = f"{source_prefix}:{sender}"
        data.setdefault("severity", "low")
        data["description"] = (
            msg_dict.get("body") or msg_dict.get("subject") or "(no content)"
        )

        svc_uid = self.env["zero_sudo.security.utils"]._get_service_uid(
            "pager_duty.user_pager_incident_creator"
        )
        return self.with_user(svc_uid).create(data)

    @api.model
    def auto_resolve_incidents(self, source, website_id=None):
        # [@ANCHOR: auto_resolve_incidents]
        website_id = website_id or self.env.context.get("website_id")
        svc_uid = self.env["zero_sudo.security.utils"]._get_service_uid(
            "pager_duty.user_pager_service_internal"
        )
        IncidentModel = self.env["pager.incident"].with_user(svc_uid)

        domain = [("source", "=", source), ("status", "in", ["open", "acknowledged"])]
        if website_id:
            domain.append(("website_id", "=", website_id))

        open_incidents = IncidentModel.search(domain, limit=1000)

        if open_incidents:
            open_incidents.write({"status": "resolved"})
            mail_svc = self.env["zero_sudo.security.utils"]._get_service_uid(
                "zero_sudo.mail_service_internal"
            )
            msg_body = _("Auto-resolved by NOC monitor recovery sequence.")
            for incident in open_incidents:
                incident.with_user(mail_svc).message_post(
                    body=msg_body)   # fmt: skip
        return True

    @api.model_create_multi
    def create(self, vals_list):
        records = super(PagerIncident, self.with_context(mail_notrack=True)).create(
            vals_list
        )
        if records:
            self.env["bus.bus"]._sendone("pager_duty", "update_board", {})
        return records

    def action_acknowledge(self):
        # [@ANCHOR: action_acknowledge_incident]
        self.write({"status": "acknowledged"})
        return True

    @api.model
    def get_board_data(self):
        # [@ANCHOR: pager_board_data]
        # Performance optimization: Use Postgres procedure to fetch dashboard data in one round-trip.
        # [@ANCHOR: pager_board_stats]
        website_id = self.env.context.get("website_id")
        if not website_id:
            current_website = self.env["website"].get_current_website()
            if current_website:
                website_id = current_website.id
        
        company_id = self.env.company.id
        self.env.cr.execute("SELECT pager_get_board_data(%s, %s)", [website_id, company_id])
        return self.env.cr.fetchone()[0]

    # [@ANCHOR: pager_mcp_triage_tools]
    # PAGER_DUTY_MCP_AI_TRIAGE.md's "Real build order" slice 1: the three
    # genuinely non-destructive tools (list_incidents/get_incident/
    # add_incident_note) an AI triage MCP server can safely call, backed by
    # a narrowly-scoped, read-only-on-pager.incident service account
    # (group_pager_mcp_triage_service, see security.xml) rather than raw
    # ORM access. Deliberately three plain, queryable/callable model
    # methods, not baked into the MCP server module itself, so they're
    # testable directly against the real service account's real ACL
    # (see test_pager_mcp_triage.py) without needing a live MCP transport.
    # set_incident_status (acknowledge/resolve) is deliberately NOT here --
    # see PAGER_DUTY_MCP_AI_TRIAGE.md's own slice 1a for why that one needs
    # its own explicit go/no-go before it's ever built.

    @api.model
    def mcp_list_incidents(self, status=None, severity=None, limit=50):
        """Read-only summary list for the MCP triage server's list_incidents
        tool. Callable directly under the narrowly-scoped
        group_pager_mcp_triage_service account -- no elevation needed, this
        is exactly what that account's own read-only ACL already covers."""
        domain = []
        if status:
            domain.append(("status", "=", status))
        if severity:
            domain.append(("severity", "=", severity))
        incidents = self.search(domain, order="create_date desc", limit=limit)
        return [
            {
                "id": inc.id,
                "name": inc.name,
                "source": inc.source,
                "severity": inc.severity,
                "status": inc.status,
                "occurrence_count": inc.occurrence_count,
                "is_escalated": inc.is_escalated,
                "create_date": inc.create_date.isoformat() if inc.create_date else None,
                "last_occurred": inc.last_occurred.isoformat() if inc.last_occurred else None,
            }
            for inc in incidents
        ]

    def mcp_get_incident_detail(self):
        """Full detail for the MCP triage server's get_incident tool --
        source/severity/description/status plus a simplified chatter
        history (prior messages/advice already posted, including by the AI
        itself on a previous pass). Reads message_ids under the same
        zero_sudo.mail_service_internal account report_incident()'s own
        escalation/creation notices already post through, rather than
        granting the calling MCP service account its own mail.message ACL
        -- the caller only ever needs read access to THIS model, not to
        mail.message generally."""
        self.ensure_one()
        mail_svc = self.env["zero_sudo.security.utils"]._get_service_uid(
            "zero_sudo.mail_service_internal"
        )
        # Sorted by id, not date: mail.message.date is second-resolution,
        # so two messages posted within the same second (a real
        # possibility for automated report_incident()/mcp_add_note()
        # traffic, not just a test artifact) can tie -- id is strictly
        # increasing on insert and gives a reliable chronological order
        # regardless.
        messages = self.with_user(mail_svc).message_ids.sorted(key=lambda m: m.id)
        return {
            "id": self.id,
            "name": self.name,
            "source": self.source,
            "severity": self.severity,
            "description": self.description,
            "status": self.status,
            "occurrence_count": self.occurrence_count,
            "is_escalated": self.is_escalated,
            "create_date": self.create_date.isoformat() if self.create_date else None,
            "last_occurred": self.last_occurred.isoformat() if self.last_occurred else None,
            "messages": [
                {
                    "author": m.author_id.name or m.email_from or "",
                    "date": m.date.isoformat() if m.date else None,
                    "body": m.body,
                }
                for m in messages
                if m.message_type in ("comment", "notification")
            ],
        }

    def mcp_add_note(self, text):
        """Posts `text` to this incident's own chatter for the MCP triage
        server's add_incident_note tool, tagged so it's visually
        distinguishable as AI-authored -- same convention
        action_escalate_unacknowledged's own "🚨 ESCALATION" prefix already
        established for automated messages. Posts under
        zero_sudo.mail_service_internal, the same elevated account every
        other message_post() call on this model already uses -- the
        calling MCP service account never needs write access to this
        model or mail.message ACL of its own to post a note."""
        self.ensure_one()
        mail_svc = self.env["zero_sudo.security.utils"]._get_service_uid(
            "zero_sudo.mail_service_internal"
        )
        self.with_user(mail_svc).message_post(body=_("🤖 AI Triage: %s", text))
        return True
