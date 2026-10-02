# SPDX-License-Identifier: AGPL-3.0-or-later

import datetime
import logging
from odoo import _, fields, models, api

from .incident import TREND_TRACKED_SEVERITIES
from .inbound_spam_filter import detect_inbound_spam_signals

_logger = logging.getLogger(__name__)

# The stage value a flagged inbound email's mirrored hams_helpdesk.ticket
# is routed to instead of the default "new" -- per Bruce's own decision
# (night_shift_questions/answered/inbound-spam-filter-location-and-signal-
# e14a6f8b.md): filter at mail-ingestion time, in this adapter, but never
# silently drop a message -- a flagged ticket still exists, still shows up
# in the ordinary Tickets list, and a human can move it straight back to
# "new" (the stage field is a clickable statusbar) to recover a false
# positive.
_SPAM_QUARANTINE_STAGE = "spam"
# Only incidents created from a real inbound email carry this prefix (see
# incident.py's own message_new(), [@ANCHOR: pager_incident_message_new]:
# data["source"] = f"{source_prefix}:{sender}", source_prefix defaults to
# "email"). Monitoring/synthetic incidents use other sources entirely
# (e.g. "test_source", a pager_check's own check name) -- gating on this
# prefix keeps the spam heuristic scoped to genuine inbound mail only, so
# it can never misclassify a monitoring signal.
_EMAIL_SOURCE_PREFIX = "email:"


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
        # Dynamically resolved, same spirit as target_model itself just
        # above: a future/alternate helpdesk model configured via
        # pager_duty.helpdesk_model might not define a "stage" field at
        # all, or might define one without a "spam" value -- fields_get()
        # is the generic way to check both without assuming hams_helpdesk.
        # ticket's own schema. When it's not available we still create the
        # ticket (never a silent drop), just without the stage routing.
        stage_field_info = target_env.fields_get(allfields=["stage"]).get("stage")
        stage_supports_spam = bool(stage_field_info) and any(
            key == _SPAM_QUARANTINE_STAGE
            for key, _label in stage_field_info.get("selection", [])
        )

        payloads = []
        spam_reasons_by_incident = {}
        for incident in incidents_to_process:
            spam_reasons = []
            if incident.source and incident.source.startswith(_EMAIL_SOURCE_PREFIX):
                spam_reasons = detect_inbound_spam_signals(
                    subject=incident.name,
                    body_html=incident.description,
                )
            if spam_reasons:
                spam_reasons_by_incident[incident.id] = spam_reasons
                _logger.info(
                    "Inbound-mail incident %s (source=%s) flagged as likely spam/"
                    "phishing by the mail-ingestion filter: %s",
                    incident.name,
                    incident.source,
                    "; ".join(spam_reasons),
                )
                incident.message_post(
                    body=_(
                        "Flagged as likely spam/phishing by the automated "
                        "mail-ingestion filter. The mirrored helpdesk ticket "
                        "was routed to quarantine instead of the on-duty "
                        "admin. Reasons: %s"
                    )
                    % "; ".join(spam_reasons),
                    subtype_xmlid="mail.mt_note",
                )

            # A flagged message still gets the ordinary on-duty assignee
            # here (hams_helpdesk.ticket's own create() independently
            # re-resolves and fills in the on-duty admin whenever a
            # payload omits "user_id" at all -- see its own
            # [@ANCHOR: COMM_helpdesk_ticket_creation] -- so leaving this
            # unset would not actually prevent the assignment, only hide
            # it from this payload). What this adapter DOES fully control
            # is the stage routing below and the calendar block skip a few
            # lines down: a flagged message never gets an "Incident
            # Response" calendar meeting scheduled over junk, even though
            # it still shows an owner for audit/visibility purposes.
            assignee_id = _assignee_for(incident.website_id.id)
            description = f"<p><strong>Severity:</strong> {incident.severity}</p><p>{incident.description or 'No description provided.'}</p>"
            if spam_reasons:
                reasons_html = "".join(f"<li>{reason}</li>" for reason in spam_reasons)
                description = (
                    "<p><strong>⚠ Possible spam/phishing</strong> "
                    "(flagged by the automated mail-ingestion filter):</p>"
                    f"<ul>{reasons_html}</ul>"
                ) + description
            payload = {
                "name": f"[PAGER] {incident.name}",
                "description": description,
            }
            if assignee_id:
                payload["user_id"] = assignee_id
            if spam_reasons and stage_supports_spam:
                payload["stage"] = _SPAM_QUARANTINE_STAGE
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
            if incident.id in spam_reasons_by_incident:
                continue
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
