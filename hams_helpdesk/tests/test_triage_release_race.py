# Copyright © Bruce Perens K6BP.
# SPDX-License-Identifier: AGPL-3.0-or-later
# -*- coding: utf-8 -*-
"""A held ticket released by two transactions at once (the release cron and a triage verdict) is announced once."""
import psycopg2.errors
from odoo.sql_db import db_connect
from odoo.tests.common import tagged
from odoo.tools import mute_logger

from odoo.addons.zero_sudo.tests.real_transaction import RealTransactionCase


@tagged("post_install", "-at_install", "standard")
class TestTriageReleaseRace(RealTransactionCase):
    # Tests [@ANCHOR: hams_helpdesk:triage_release_notification]

    def setUp(self):
        super().setUp()
        self.Ticket = self.env["hams_helpdesk.ticket"]
        ticket = self.Ticket.create({"name": "Release race", "description": "<p>x</p>", "ticket_type": "general"})
        ticket.write({"notify_state": "held"})
        self.env.cr.commit()
        self.ticket_id = ticket.id
        self.addCleanup(self._drop_ticket)
        self.routed = []
        model = type(self.Ticket)
        real = model._automated_routing_and_notification
        routed = self.routed

        def counting(records):
            routed.extend(records.ids)
            return real(records)

        self.safe_patch_object(model, "_automated_routing_and_notification", counting)
        self.other = db_connect(self.env.cr.dbname).cursor()
        self.addCleanup(self.other.close)

    def _drop_ticket(self):
        self.env.cr.rollback()
        self.other.rollback()
        self.env.invalidate_all()
        self.Ticket.browse(self.ticket_id).unlink()
        self.env.cr.commit()

    def _release(self):
        self.env.invalidate_all()
        self.Ticket.browse(self.ticket_id)._triage_release_notification()

    def test_a_release_in_flight_elsewhere_sends_nothing_here(self):
        """Transaction A (the verdict) has written "sent" and not committed; B (the cron) must skip the row, not wait for it."""
        self.other.execute("UPDATE hams_helpdesk_ticket SET notify_state = 'sent' WHERE id = %s", (self.ticket_id,))
        self.env.cr.execute("SET LOCAL lock_timeout = '3s'")
        self._release()
        self.assertEqual(self.routed, [])
        self.other.commit()
        self.env.cr.rollback()
        self._release()
        self.assertEqual(self.routed, [], "once A has committed the ticket is sent: nothing more goes out")

    def test_a_release_committed_after_this_one_looked_sends_nothing_here(self):
        """B read the ticket as held, then A committed "sent". The claim hits a serialization failure (Odoo retries the
        request, and the retry sees "sent"); the point is that nothing is routed from B's stale view."""
        ticket = self.Ticket.browse(self.ticket_id)
        self.assertEqual(ticket.notify_state, "held")
        self.other.execute("UPDATE hams_helpdesk_ticket SET notify_state = 'sent' WHERE id = %s", (self.ticket_id,))
        self.other.commit()
        with self.assertRaises(psycopg2.errors.SerializationFailure), mute_logger("odoo.sql_db"):
            ticket._triage_release_notification()
        self.assertEqual(self.routed, [])
        self.env.cr.rollback()
        self._release()
        self.assertEqual(self.routed, [], "the retry sees \"sent\"")

    def test_a_free_held_ticket_is_released_once(self):
        self._release()
        self._release()
        self.assertEqual(self.routed, [self.ticket_id])
