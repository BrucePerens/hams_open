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
    website_id = fields.Many2one("website", string="Website", ondelete="cascade")
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
        string="MTTA (Minutes)", readonly=True, help="Mean Time To Acknowledge"
    )
    mttr = fields.Float(
        string="MTTR (Minutes)", readonly=True, help="Mean Time To Resolve"
    )
    helpdesk_ticket_id = fields.Integer(
        string="Helpdesk Ticket ID",
        help="Stores the integer ID of the generated helpdesk ticket to remain schema-agnostic.",
        tracking=True,
    )
    helpdesk_ticket_model = fields.Char(
        string="Ticket Model",
        help="The Odoo model used for the ticket (e.g. hams_helpdesk.ticket or helpdesk.ticket).",
    )

    def write(self, vals):
        now = fields.Datetime.now()
        if vals.get("status") == "acknowledged":
            vals["time_acknowledged"] = now
            if not vals.get("acknowledged_by_id"):
                vals["acknowledged_by_id"] = self.env.user.id
        elif vals.get("status") == "resolved":
            vals["time_resolved"] = now

        res = super(
            PagerIncident, self.with_context(mail_notrack=True)
        ).write(vals)

        # ADR 0078: O(1) Memory Mapping / Event Bus Optimization
        for rec in self:
            if rec.time_acknowledged and rec.create_date and not rec.mtta:
                rec.mtta = (
                    rec.time_acknowledged - rec.create_date
                ).total_seconds() / 60.0
            if rec.time_resolved and rec.create_date and not rec.mttr:
                rec.mttr = (rec.time_resolved - rec.create_date).total_seconds() / 60.0

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
        # to ensure minimum privilege. We use the mail service internal user to execute
        # message_post and search.
        mail_svc = self.env["zero_sudo.security.utils"]._get_service_uid(
            "zero_sudo.mail_service_internal"
        )
        IncidentModel = self.env["pager.incident"].with_user(mail_svc)

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
                lambda u: not inc.website_id or ('website_id' in u._fields and u.website_id == inc.website_id)
            ).mapped("partner_id")

            if not partners:
                partners = pager_admin_group.user_ids.mapped("partner_id")

            msg_body = _("🚨 ESCALATION: Incident open for > 15 minutes!")
            inc.with_user(mail_svc).message_post(body=msg_body, partner_ids=partners.ids)   # fmt: skip
        incidents.write({"is_escalated": True})

    @api.model
    def report_incident(self, vals):
        """
        Reports a new incident. Supports multi-website partitioning.
        """
        # [@ANCHOR: report_incident_rate_limit]
        source = vals.get("source", "unknown")
        website_id = vals.get("website_id") or self.env.context.get("website_id")
        if not website_id and hasattr(self.env, "website") and self.env.website:
            website_id = self.env.website.id
        # [@ANCHOR: pd_redis_rate_limit]
        redis_key = f"pager_rate_limit:{source}:{website_id or 'global'}"

        if redis and redis_pool:
            try:
                r_client = redis.Redis(connection_pool=redis_pool)
                # SET with NX=True and EX=60 provides an atomic rate limit check-and-set
                if not r_client.set(redis_key, "1", ex=60, nx=True):
                    return False
            except (redis.exceptions.RedisError, Exception) as e: # audit-ignore-catch-all
                _logger.warning("Redis rate limit check failed: %s", e)

        svc_uid = self.env["zero_sudo.security.utils"]._get_service_uid(
            "pager_duty.user_pager_service_internal"
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
            return existing.id

        if vals.get("name", "New") == "New":
            vals["name"] = "INC-AUTO"

        incident = IncidentModel.create(vals)
        on_duty_user = self.env["calendar.event"].with_context(
            website_id=website_id
        ).get_current_on_duty_admin()

        # Suppress native pager notifications if helpdesk integration is active
        # to prevent duplicate alerting (Helpdesk will handle the page).
        use_helpdesk = self.env["zero_sudo.security.utils"]._get_system_param("pager_duty.helpdesk_model")

        if on_duty_user and not use_helpdesk:
            mail_svc = self.env["zero_sudo.security.utils"]._get_service_uid(
                "zero_sudo.mail_service_internal"
            )
            msg_body = _("New Incident Created")
            partner_ids = [on_duty_user.partner_id.id]
            incident.with_user(mail_svc).message_post(body=msg_body, partner_ids=partner_ids)   # fmt: skip
        return incident.id

    @api.model
    def auto_resolve_incidents(self, source):
        # [@ANCHOR: auto_resolve_incidents]
        svc_uid = self.env["zero_sudo.security.utils"]._get_service_uid(
            "pager_duty.user_pager_service_internal"
        )
        IncidentModel = self.env["pager.incident"].with_user(svc_uid)

        open_incidents = IncidentModel.search(
            [("source", "=", source), ("status", "in", ["open", "acknowledged"])],
            limit=1000,
        )

        if open_incidents:
            open_incidents.write({"status": "resolved"})
            mail_svc = self.env["zero_sudo.security.utils"]._get_service_uid(
                "zero_sudo.mail_service_internal"
            )
            msg_body = _("Auto-resolved by NOC monitor recovery sequence.")
            for incident in open_incidents:
                incident.with_user(mail_svc).message_post(body=msg_body)   # fmt: skip
        return True

    @api.model_create_multi
    def create(self, vals_list):
        records = super(
            PagerIncident, self.with_context(mail_notrack=True)
        ).create(vals_list)
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
        on_duty_user = self.env["calendar.event"].get_current_on_duty_admin()
        duty_name = on_duty_user.name if on_duty_user else "None"

        domain = [("status", "in", ["open", "acknowledged"])]
        check_domain = []
        if self.env.context.get("website_id"):
            domain.append(("website_id", "=", self.env.context.get("website_id")))
            check_domain.append(("website_id", "=", self.env.context.get("website_id")))

        active = self.search_read(
            domain,
            [
                "name",
                "source",
                "severity",
                "status",
                "acknowledged_by_id",
                "create_date",
            ],
            order="create_date desc",
            limit=50,
        )

        for a in active:
            a["ack_name"] = (
                a["acknowledged_by_id"][1] if a["acknowledged_by_id"] else False
            )

        res_domain = [("status", "=", "resolved")]
        if self.env.context.get("website_id"):
            res_domain.append(("website_id", "=", self.env.context.get("website_id")))

        resolved = self.search_read(
            res_domain,
            ["name", "source", "severity", "time_resolved"],
            order="time_resolved desc",
            limit=10,
        )

        # [@ANCHOR: pager_board_stats]
        # Aggregate stats for the health summary component
        svc_uid = self.env["zero_sudo.security.utils"]._get_service_uid(
            "pager_duty.user_pager_service_internal"
        )
        checks = self.env["pager.check"].with_user(svc_uid).read_group(
            check_domain, ["status"], ["status"]
        )
        stats = {"passing": 0, "failing": 0, "maintenance": 0}
        for c in checks:
            if c["status"] in stats:
                stats[c["status"]] = c["status_count"]

        return {
            "on_duty": duty_name,
            "active": active,
            "resolved": resolved,
            "stats": stats
        }
