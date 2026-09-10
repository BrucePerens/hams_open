# This software is distributed under the terms of the Affero General Public License (AGPL-3).
# SPDX-License-Identifier: AGPL-3.0-or-later

import datetime
import logging
from odoo import _, fields, models, api

from .incident import TREND_TRACKED_SEVERITIES

_logger = logging.getLogger(__name__)


class PagerDutyIncidentTicketAdapter(models.Model):
    _inherit = "pager.incident"

    @api.model_create_multi
    # [@ANCHOR: pager_duty:incident_ticket_adapter_create]
    def create(self, vals_list):
        records = super().create(vals_list)
        # Bug-hunt fix: this used to call action_generate_helpdesk_ticket()
        # on every newly created incident regardless of severity, silently
        # defeating incident.py's own trend-detection design -- its
        # TREND_TRACKED_SEVERITIES gate deliberately does NOT page on_duty
        # immediately for low/medium severities (see report_incident()'s
        # own "does not page on-duty immediately" comment), but a real
        # Helpdesk ticket PLUS a 1-hour "Incident Response" calendar block
        # on the on-duty admin's own calendar were still being generated
        # for every one of them the instant Helpdesk integration is
        # active -- which _notify_on_duty()'s own comment says IS the
        # normal deployment mode ("Helpdesk will handle the page"). A
        # "Trend:" incident raised once low/medium occurrences actually
        # cross the threshold is unaffected: _raise_trend_incident()
        # always creates those with severity="high", so they still get a
        # real ticket. action_generate_helpdesk_ticket() itself is left
        # ungated -- an explicit, direct call (e.g. a manual admin action)
        # should still work regardless of severity.
        records.filtered(
            lambda r: r.severity not in TREND_TRACKED_SEVERITIES
        ).action_generate_helpdesk_ticket()
        return records

    def action_generate_helpdesk_ticket(self):
        """
        Adapter API: Dynamically resolves the helpdesk model from system parameters
        and dispatches a standard payload. Includes an emergency SMTP fallback.
        """
        # [@ANCHOR: pd_helpdesk_adapter]
        incidents_to_process = self.filtered(lambda r: not r.helpdesk_ticket_id)
        if not incidents_to_process:
            return

        target_model = self.env["zero_sudo.security.utils"]._get_system_param(
            "pager_duty.helpdesk_model", default="hams_helpdesk.ticket"
        )

        # Bug-hunt fix: get_current_on_duty_admin() resolves against
        # env.context["website_id"] (falling back to the ambient "current
        # website" otherwise -- see schedule.py), so a single lookup for
        # the WHOLE batch used to assign every incident in a mixed-website
        # batch to whichever one admin happened to be on duty for a single
        # (often unrelated) website, and calendar-block only that admin --
        # not necessarily the admin actually on duty for a given
        # incident's own website. Resolved per incident's own website_id
        # instead, memoized since incidents commonly share one. Mirrors
        # _notify_on_duty() in incident.py, which already scopes this
        # correctly.
        assignee_by_website = {}

        def _assignee_for(website_id):
            if website_id not in assignee_by_website:
                user = (
                    self.env["calendar.event"]
                    .with_context(website_id=website_id)
                    .get_current_on_duty_admin()
                )
                assignee_by_website[website_id] = user.id if user else False
            return assignee_by_website[website_id]

        if target_model not in self.env:
            _logger.warning(
                "Target helpdesk model '%s' not found. Ensure the module is installed.",
                target_model,
            )
            for incident in incidents_to_process:
                self._execute_smtp_fallback(
                    incident,
                    "Target model not installed.",
                    _assignee_for(incident.website_id.id),
                )
            return
        target_env = self.env[target_model]

        payloads = []
        for incident in incidents_to_process:
            assignee_id = _assignee_for(incident.website_id.id)
            payload = {
                "name": f"[PAGER] {incident.name}",
                "description": f"<p><strong>Severity:</strong> {incident.severity}</p><p>{incident.description or 'No description provided.'}</p>",
            }
            if assignee_id:
                payload["user_id"] = assignee_id
            payloads.append(payload)

        # Isolate specific module service accounts for scoped relational object generation
        hd_uid = self.env["zero_sudo.security.utils"]._get_service_uid(
            "hams_helpdesk.user_helpdesk_service"
        )
        tickets = target_env.with_user(hd_uid).with_context(
            mail_create_nosubscribe=True, mail_create_nolog=True, mail_auto_subscribe_no_notify=True
        ).create(payloads)

        for incident, ticket in zip(incidents_to_process, tickets):
            incident.write(
                {"helpdesk_ticket_id": ticket.id, "helpdesk_ticket_model": target_model}
            )

        calendar_payloads = []
        for incident, ticket in zip(incidents_to_process, tickets):
            assignee_id = _assignee_for(incident.website_id.id)
            if not assignee_id:
                continue
            user = self.env["res.users"].browse(assignee_id)
            if not user.partner_id:
                continue
            calendar_payloads.append({
                "name": f"Incident Response: {incident.name}",
                "start": fields.Datetime.now(),
                "stop": fields.Datetime.now() + datetime.timedelta(hours=1),
                "partner_ids": [(4, user.partner_id.id)],
                "description": f"Auto-generated by PagerDuty incident escalation for ticket #{ticket.id}.",
            })
        if calendar_payloads:
            pd_uid = self.env["zero_sudo.security.utils"]._get_service_uid(
                "pager_duty.user_pager_service_internal"
            )
            self.env["calendar.event"].with_user(pd_uid).create(calendar_payloads)

    # [@ANCHOR: pager_duty:execute_smtp_fallback]
    def _execute_smtp_fallback(self, incident, error_msg, assignee_id=False):
        """
        Executes a direct SMTP page if the Helpdesk integration fails or is unreachable.
        """
        if not assignee_id:
            # Bug-hunt fix: scope to this incident's own website, same
            # reasoning as action_generate_helpdesk_ticket() above -- an
            # unscoped lookup can resolve to the wrong website's on-duty
            # admin (or none) rather than this specific incident's own.
            user = (
                self.env["calendar.event"]
                .with_context(website_id=incident.website_id.id)
                .get_current_on_duty_admin()
            )
            if user:
                assignee_id = user.id

        pd_uid = self.env["zero_sudo.security.utils"]._get_service_uid(
            "pager_duty.user_pager_service_internal"
        )

        body = (
            f"🚨 <b>EMERGENCY PAGE (Helpdesk Fallback)</b><br/><br/>"
            f"The Pager Duty module attempted to create a Helpdesk ticket but failed due to an internal error:<br/>"
            f"<i>{error_msg}</i><br/><br/>"
            f"<b>Incident ID:</b> {incident.name}<br/>"
            f"<b>Severity:</b> {incident.severity}<br/>"
            f"<b>Details:</b> {incident.description}"
        )

        partner_ids = []
        if assignee_id:
            user = self.env["res.users"].browse(assignee_id)
            if user.partner_id:
                partner_ids.append(user.partner_id.id)

        incident.with_user(pd_uid).message_post(
            body=body,
            subject=_("🚨 PAGER DUTY FALLBACK: %s") % incident.name,
            partner_ids=partner_ids,
        )
