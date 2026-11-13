# Copyright © Bruce Perens K6BP.
# SPDX-License-Identifier: AGPL-3.0-or-later
# -*- coding: utf-8 -*-
"""A pager-created helpdesk ticket whose pager incident is resolved is retired before it is announced
(hams_helpdesk._triage_retire_stale); an open incident's ticket, an inbound-mail incident's ticket and a person's ticket are not."""
from odoo.tests.common import tagged
from odoo.addons.zero_sudo.tests.common import HamsTransactionCase


@tagged("post_install", "-at_install", "standard")
class TestHelpdeskTriageRetire(HamsTransactionCase):
    # Tests [@ANCHOR: pager_duty:helpdesk_ticket_triage_cleared]

    def _pair(self, status, source="backup_check"):
        ticket = self.env["hams_helpdesk.ticket"].create(
            {"name": "[PAGER] x", "description": "<p>x</p>", "ticket_type": "general"}
        )
        incident = self.env["pager.incident"].create(
            {
                "name": "x",
                "source": source,
                "severity": "high",
                "description": "x",
                "status": status,
                "helpdesk_ticket_id": ticket.id,
                "helpdesk_ticket_model": "hams_helpdesk.ticket",
            }
        )
        return ticket, incident

    def test_01_a_resolved_incident_retires_its_new_ticket_with_a_note(self):
        ticket, _incident = self._pair("resolved")
        self.assertEqual(ticket._triage_retire_stale(), ticket)
        self.assertEqual(ticket.stage, "resolved")
        self.assertEqual(ticket.notify_state, "suppressed")
        self.assertIn("pager incident", ticket.message_ids.sorted(key=lambda m: m.id)[-1].body)

    def test_02_an_open_incident_keeps_its_ticket(self):
        ticket, _incident = self._pair("open")
        self.assertFalse(ticket._triage_retire_stale())
        self.assertEqual(ticket.stage, "new")

    def test_03_a_ticket_from_inbound_mail_is_never_retired(self):
        ticket, _incident = self._pair("resolved", source="info-email:someone@example.org")
        self.assertFalse(ticket._triage_retire_stale())
        self.assertEqual(ticket.stage, "new")

    def test_04_a_ticket_with_no_incident_is_never_retired(self):
        ticket = self.env["hams_helpdesk.ticket"].create({"name": "Human", "description": "<p>x</p>"})
        self.assertFalse(ticket._triage_retire_stale())
        self.assertEqual(ticket.stage, "new")

    def test_05_a_ticket_shared_with_an_unresolved_incident_is_kept(self):
        ticket, _incident = self._pair("resolved")
        self.env["pager.incident"].create(
            {
                "name": "y",
                "source": "other_check",
                "severity": "high",
                "description": "y",
                "status": "open",
                "helpdesk_ticket_id": ticket.id,
                "helpdesk_ticket_model": "hams_helpdesk.ticket",
            }
        )
        self.assertFalse(ticket._triage_retire_stale())
        self.assertEqual(ticket.stage, "new")
