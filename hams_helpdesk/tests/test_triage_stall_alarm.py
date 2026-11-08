# Copyright © Bruce Perens K6BP.
# SPDX-License-Identifier: AGPL-3.0-or-later
# -*- coding: utf-8 -*-
"""The triage review delay is measured, and a review that stalls raises an alarm (Bruce, 2026-11-02, night_shift_todo
ticket-triage-agent-reviews-tickets-on-submission-and-marks-spam, item 1): a stalled review must not leave tickets
unjudged silently."""
from datetime import timedelta

from odoo import fields
from odoo.tests.common import tagged
from odoo.addons.zero_sudo.tests.common import HamsTransactionCase


@tagged("post_install", "-at_install", "standard")
class TestTriageStallAlarm(HamsTransactionCase):
    def setUp(self):
        super().setUp()
        self.Ticket = self.env["hams_helpdesk.ticket"]
        self.agent_uid = self.env["zero_sudo.security.utils"]._get_service_uid("hams_helpdesk.user_ai_triage_service")
        self.settings = {}
        utils_model = type(self.env["zero_sudo.security.utils"])
        real = utils_model._get_system_param
        settings = self.settings

        def answer(utils, key, default=None):
            return settings[key] if key in settings else real(utils, key, default)

        self.safe_patch_object(utils_model, "_get_system_param", answer, create=True)
        self.reports = []
        reports = self.reports

        def record(model, units, reporter, message=""):
            reports.append((list(units), reporter, message))
            return {}

        self.safe_patch_object(
            type(self.env["hams_helpdesk.exception.fingerprint"]), "_report_unit_failures", record, create=True
        )

    def _ticket(self, age_seconds, name="Waiting for a verdict", **extra):
        ticket = self.Ticket.create({"name": name, "description": "<p>x</p>", "ticket_type": "general", **extra})
        created = fields.Datetime.now() - timedelta(seconds=age_seconds)
        self.env.cr.execute("UPDATE hams_helpdesk_ticket SET create_date = %s WHERE id = %s", (created, ticket.id))
        ticket.invalidate_recordset(["create_date"])
        return ticket

    def test_01_the_alarm_is_off_by_default(self):
        # Tests [@ANCHOR: hams_helpdesk:cron_triage_stall_alarm]
        self._ticket(3600)
        self.assertEqual(self.Ticket._cron_triage_stall_alarm(), 0)
        self.assertEqual(self.reports, [])

    def test_02_a_ticket_with_no_verdict_past_the_threshold_raises_the_alarm_once(self):
        # Tests [@ANCHOR: hams_helpdesk:cron_triage_stall_alarm]
        self.settings["hams_helpdesk.triage_stall_seconds"] = "120"
        stalled = self._ticket(300)
        fresh = self._ticket(30, "Still inside the allowed time")
        self.assertEqual(self.Ticket._cron_triage_stall_alarm(), 1)
        self.assertTrue(stalled.triage_stalled_at)
        self.assertFalse(fresh.triage_stalled_at)
        self.assertEqual(len(self.reports), 1)
        units, reporter, message = self.reports[0]
        self.assertEqual(units, ["ticket.triage.event.service"])
        self.assertEqual(reporter, "triage stall alarm")
        self.assertIn("1 ticket(s) waited more than 120 seconds", message)
        self.assertEqual(self.Ticket._cron_triage_stall_alarm(), 0, "the same ticket never alarms twice")
        self.assertEqual(len(self.reports), 1)

    def test_03_judged_suspicious_old_and_other_stage_tickets_do_not_alarm(self):
        # Tests [@ANCHOR: hams_helpdesk:cron_triage_stall_alarm]
        self.settings["hams_helpdesk.triage_stall_seconds"] = "120"
        judged = self._ticket(300, "Judged")
        judged.with_user(self.agent_uid).mcp_record_verdict("real", "A member")
        picked_up = self._ticket(300, "Picked up")
        picked_up.write({"stage": "in_progress"})
        old = self._ticket(3 * 86400, "Older than the lookback")
        suspicious = self._ticket(300, "Hidden content")
        # write() recomputes the flag from the text, so set it the way the filter does when it finds hidden content.
        self.env.cr.execute("UPDATE hams_helpdesk_ticket SET suspicious = TRUE WHERE id = %s", (suspicious.id,))
        suspicious.invalidate_recordset(["suspicious"])
        server_exception = self._ticket(300, "Exception", ticket_type="server_exception")
        self.assertEqual(self.Ticket._cron_triage_stall_alarm(), 0)
        self.assertEqual(self.reports, [])
        for ticket in (judged, picked_up, old, suspicious, server_exception):
            self.assertFalse(ticket.triage_stalled_at, ticket.name)

    def test_04_the_delay_from_creation_to_the_agents_verdict_is_recorded(self):
        # Tests [@ANCHOR: hams_helpdesk:triage_record_verdict]
        ticket = self._ticket(45)
        ticket.with_user(self.agent_uid).mcp_record_verdict("real", "A member")
        self.assertGreaterEqual(ticket.triage_delay_seconds, 45)
        self.assertLess(ticket.triage_delay_seconds, 45 + 120)
        rules = self.Ticket.create({"name": "Rules verdict", "description": "<p>x</p>"})
        rules._triage_record_verdict("spam", "rule hit", "rules")
        self.assertEqual(rules.triage_delay_seconds, 0, "only the agent's review delay is measured")

    def test_05_no_caller_can_set_the_alarm_or_delay_fields(self):
        # Tests [@ANCHOR: hams_helpdesk:triage_record_verdict]
        forged = self.Ticket.create(
            {"name": "Forged", "description": "<p>x</p>", "triage_stalled_at": fields.Datetime.now(), "triage_delay_seconds": 5}
        )
        self.assertFalse(forged.triage_stalled_at)
        self.assertFalse(forged.triage_delay_seconds)
