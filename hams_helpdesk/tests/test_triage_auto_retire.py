# Copyright © Bruce Perens K6BP.
# SPDX-License-Identifier: AGPL-3.0-or-later
# -*- coding: utf-8 -*-
"""Automatic stale retirement (Bruce, 2026-11-13): a New server-exception ticket whose failure has cleared is resolved with a
note before anyone is told about it; one that still fires is announced as always; a person's ticket is never touched."""
from datetime import timedelta

from odoo import fields
from odoo.tests.common import tagged
from odoo.addons.zero_sudo.tests.common import HamsTransactionCase

from .bulk_mail_fixtures import NotificationWatch


@tagged("post_install", "-at_install", "standard")
class TestTriageAutoRetire(HamsTransactionCase):
    # Tests [@ANCHOR: hams_helpdesk:triage_cleared]
    # Tests [@ANCHOR: hams_helpdesk:triage_retire_stale]
    # Tests [@ANCHOR: hams_helpdesk:triage_sweep_stale]
    @classmethod
    def setUpClass(cls):
        super().setUpClass()
        company = cls.env.ref("base.main_company")
        alias_domain = cls.env["mail.alias.domain"].search([("name", "=", "hams.com")], limit=1)
        if not alias_domain:
            alias_domain = cls.env["mail.alias.domain"].create({"name": "hams.com"})
        if company.alias_domain_id != alias_domain:
            company.alias_domain_id = alias_domain

    def setUp(self):
        super().setUp()
        self.Ticket = self.env["hams_helpdesk.ticket"]
        admin = self.env.ref("base.user_admin")
        admin.email = "on-duty-admin@example.org"
        self.on_duty = admin
        self.safe_patch_object(type(self.env["calendar.event"]), "get_current_on_duty_admin", lambda _self: admin, create=True)
        self.settings = {"hams_helpdesk.triage_notify_hold_seconds": "20"}
        utils_model = type(self.env["zero_sudo.security.utils"])
        real = utils_model._get_system_param
        settings = self.settings

        def answer(utils, key, default=None):
            return settings[key] if key in settings else real(utils, key, default)

        self.safe_patch_object(utils_model, "_get_system_param", answer, create=True)

    def _exception_ticket(self, hours_since_seen, name="Daemon failure: nz.rsm.sync.service"):
        ticket = self.Ticket.create({"name": name, "description": "<p>unit failed</p>", "ticket_type": "server_exception"})
        self.env["hams_helpdesk.exception.fingerprint"].create(
            {
                "name": name,
                "fingerprint": "fp-%s" % ticket.id,
                "ticket_id": ticket.id,
                "last_seen": fields.Datetime.now() - timedelta(hours=hours_since_seen),
            }
        )
        return ticket

    def _window_over(self, ticket):
        ticket.write({"notify_release_at": fields.Datetime.now() - timedelta(seconds=1)})

    def test_01_a_server_exception_ticket_is_held_for_the_triage_review(self):
        ticket = self._exception_ticket(0)
        self.assertEqual(ticket.notify_state, "held")

    def test_02_a_cleared_exception_ticket_is_resolved_with_a_note_and_nobody_is_mailed(self):
        ticket = self._exception_ticket(7)
        watch = NotificationWatch(self)
        self._window_over(ticket)
        self.Ticket._cron_release_held_notifications()
        self.assertEqual(ticket.stage, "resolved")
        self.assertEqual(ticket.notify_state, "suppressed")
        note = ticket.message_ids.sorted(key=lambda m: m.id)[-1]
        self.assertIn("Resolved automatically", note.body)
        self.assertTrue(note.subtype_id.internal)
        self.assertEqual([(m.subject, m.email_to) for m in watch.mails()], [])
        self.assertEqual(len(watch.notifications()), 0)
        self.assertEqual(watch.bus, [])

    def test_03_a_verdict_arriving_for_a_cleared_ticket_does_not_announce_it(self):
        ticket = self._exception_ticket(7)
        watch = NotificationWatch(self)
        ticket._triage_release_notification()
        self.assertEqual(ticket.stage, "resolved")
        self.assertEqual(watch.bus, [])
        self.assertEqual(len(watch.notifications()), 0)

    def test_04_an_exception_ticket_that_still_fires_is_announced_as_before(self):
        ticket = self._exception_ticket(1)
        watch = NotificationWatch(self)
        self._window_over(ticket)
        self.Ticket._cron_release_held_notifications()
        self.assertEqual(ticket.stage, "new")
        self.assertEqual(ticket.notify_state, "sent")
        self.assertGreater(watch.count(), 0, "the control: a firing exception pages")

    def test_05_a_ticket_a_person_filed_is_never_retired(self):
        ticket = self.Ticket.create({"name": "My rotator is stuck", "description": "<p>x</p>", "ticket_type": "general"})
        self.env.cr.execute("UPDATE hams_helpdesk_ticket SET create_date = %s WHERE id = %s", (fields.Datetime.now() - timedelta(days=3), ticket.id))
        ticket.invalidate_recordset()
        self.assertEqual(self.Ticket._triage_sweep_stale(), 0)
        self._window_over(ticket)
        self.Ticket._cron_release_held_notifications()
        self.assertEqual(ticket.stage, "new")
        self.assertEqual(ticket.notify_state, "sent")

    def test_06_the_sweep_resolves_an_old_new_exception_ticket_that_was_already_announced(self):
        self.settings["hams_helpdesk.triage_notify_hold_seconds"] = "0"
        ticket = self._exception_ticket(9)
        self.assertEqual(ticket.notify_state, "sent")
        self.assertEqual(self.Ticket._triage_sweep_stale(), 1)
        self.assertEqual(ticket.stage, "resolved")

    def test_07_a_ticket_a_person_already_picked_up_is_left_alone(self):
        ticket = self._exception_ticket(9)
        ticket.write({"stage": "in_progress"})
        self.assertEqual(self.Ticket._triage_sweep_stale(), 0)
        self.assertEqual(ticket.stage, "in_progress")
