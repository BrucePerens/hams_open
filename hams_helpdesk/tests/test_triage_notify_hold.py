# Copyright © Bruce Perens K6BP.
# SPDX-License-Identifier: AGPL-3.0-or-later
# -*- coding: utf-8 -*-
"""The operator notification held for the triage review (Bruce, 2026-11-02, night_shift_todo ticket-triage-agent-reviews-
tickets-on-submission-and-marks-spam, item 3): no email, toast or assignment notice for a spam ticket; a new ticket's
notification waits out a short review window and is then sent (fail open); moving a ticket out of Spam sends it."""
import base64
from datetime import timedelta

from odoo import fields
from odoo.tests.common import tagged
from odoo.addons.zero_sudo.tests.common import HamsTransactionCase

from .bulk_mail_fixtures import (
    BULK_HEADERS,
    REAL_MISSED_SPAM_NAMES,
    SES_PASS_HEADERS,
    NotificationWatch,
    raw_message,
)


@tagged("post_install", "-at_install", "standard")
class TestTriageNotifyHold(HamsTransactionCase):
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
        self.agent_uid = self.env["zero_sudo.security.utils"]._get_service_uid("hams_helpdesk.user_ai_triage_service")
        admin = self.env.ref("base.user_admin")
        admin.email = "on-duty-admin@example.org"
        self.on_duty = admin
        self.safe_patch_object(type(self.env["calendar.event"]), "get_current_on_duty_admin", lambda _self: admin, create=True)
        self.settings = {}
        utils_model = type(self.env["zero_sudo.security.utils"])
        real = utils_model._get_system_param
        settings = self.settings

        def answer(utils, key, default=None):
            if key in settings:
                return settings[key]
            return real(utils, key, default)

        self.safe_patch_object(utils_model, "_get_system_param", answer, create=True)

    def _hold(self, seconds):
        self.settings["hams_helpdesk.triage_notify_hold_seconds"] = str(seconds)

    def _new_ticket(self, name="Rotator controller will not park", **extra):
        vals = {"name": name, "description": "<p>The controller stops at 90 degrees.</p>", "ticket_type": "general"}
        vals.update(extra)
        return self.Ticket.create(vals)

    def _ingest(self, raw):
        self.Ticket.with_user(self.env.ref("hams_helpdesk.user_mail_ingest_service")).ingest_inbound_email(
            base64.b64encode(raw).decode("ascii")
        )

    def _assert_silent(self, watch, why):
        self.assertEqual([(m.subject, m.email_to) for m in watch.mails()], [], why)
        self.assertEqual(len(watch.notifications()), 0, why)
        self.assertEqual(watch.bus, [], why)

    def test_01_the_default_holds_nothing_and_a_new_ticket_notifies_at_once(self):
        # Tests [@ANCHOR: hams_helpdesk:triage_hold_seconds]
        # Tests [@ANCHOR: hams_helpdesk:COMM_triage_notification_state_on_create]
        self.assertEqual(self.Ticket._triage_hold_seconds(), 0)
        watch = NotificationWatch(self)
        ticket = self._new_ticket()
        self.assertEqual(ticket.notify_state, "sent")
        self.assertGreater(watch.count(), 0, "the control: a real ticket notifies, so the silent cases below mean something")
        self.assertTrue(any(kind == "simple_notification" for _target, kind in watch.bus))

    def test_02_the_hold_window_setting_is_bounded_and_ignores_nonsense(self):
        # Tests [@ANCHOR: hams_helpdesk:triage_hold_seconds]
        for raw, expected in (("20", 20), ("0", 0), ("", 0), ("soon", 0), ("-5", 0), ("99999", 300), (" 30 ", 30)):
            self.settings["hams_helpdesk.triage_notify_hold_seconds"] = raw
            self.assertEqual(self.Ticket._triage_hold_seconds(), expected, repr(raw))

    def test_03_a_held_ticket_mails_and_toasts_nobody_and_has_no_followers(self):
        # Tests [@ANCHOR: hams_helpdesk:COMM_triage_notification_state_on_create]
        self._hold(20)
        watch = NotificationWatch(self)
        ticket = self._new_ticket()
        self.assertEqual(ticket.notify_state, "held")
        self.assertTrue(ticket.notify_release_at)
        self.assertEqual(ticket.user_id, self.on_duty, "the ticket is assigned right away; only the announcing waits")
        self.assertFalse(ticket.message_partner_ids)
        self._assert_silent(watch, "nothing is sent during the review window")

    def test_04_a_real_verdict_releases_the_notification_at_once(self):
        # Tests [@ANCHOR: hams_helpdesk:triage_release_notification]
        # Tests [@ANCHOR: hams_helpdesk:mcp_record_verdict]
        self._hold(20)
        ticket = self._new_ticket()
        watch = NotificationWatch(self)
        ticket.with_user(self.agent_uid).mcp_record_verdict("real", "A member describing a fault")
        self.assertEqual(ticket.notify_state, "sent")
        self.assertFalse(ticket.notify_release_at)
        self.assertIn(self.on_duty.partner_id, ticket.message_partner_ids)
        self.assertTrue(any(kind == "simple_notification" for _target, kind in watch.bus), "the toast went out")
        self.assertTrue(
            any("assigned to you" in (m.body or "") for m in ticket.message_ids), "the assignment notice was posted"
        )

    def test_05_a_spam_verdict_inside_the_window_sends_nothing_ever(self):
        # Tests [@ANCHOR: hams_helpdesk:mcp_record_verdict]
        # Tests [@ANCHOR: hams_helpdesk:cron_release_held_notifications]
        self._hold(20)
        watch = NotificationWatch(self)
        ticket = self._new_ticket("Cheap supplements, order now")
        ticket.with_user(self.agent_uid).mcp_record_verdict("spam", "Bulk offer unrelated to hams.com")
        self.assertEqual((ticket.stage, ticket.notify_state), ("spam", "suppressed"))
        self.Ticket.invalidate_model()
        self.assertEqual(self.Ticket._cron_release_held_notifications(), 0)
        self._assert_silent(watch, "a ticket judged spam inside the window mails nobody, before or after the window")

    def test_06_the_window_ending_without_a_verdict_sends_anyway_fail_open(self):
        # Tests [@ANCHOR: hams_helpdesk:cron_release_held_notifications]
        # Tests [@ANCHOR: hams_helpdesk:triage_release_notification]
        self._hold(20)
        ticket = self._new_ticket()
        watch = NotificationWatch(self)
        self.assertEqual(self.Ticket._cron_release_held_notifications(), 0, "the window is still open")
        self._assert_silent(watch, "still held")
        ticket.write({"notify_release_at": fields.Datetime.now() - timedelta(seconds=1)})
        self.assertEqual(self.Ticket._cron_release_held_notifications(), 1, "the agent never answered: the notice goes out")
        self.assertEqual(ticket.notify_state, "sent")
        self.assertTrue(any(kind == "simple_notification" for _target, kind in watch.bus))
        self.assertEqual(self.Ticket._cron_release_held_notifications(), 0, "released once only")

    def test_07_a_held_ticket_a_person_puts_in_spam_is_never_announced(self):
        # Tests [@ANCHOR: hams_helpdesk:cron_release_held_notifications]
        self._hold(20)
        ticket = self._new_ticket()
        watch = NotificationWatch(self)
        ticket.write({"stage": "spam"})
        self.assertEqual(ticket.notify_state, "suppressed")
        ticket.write({"notify_state": "held", "notify_release_at": fields.Datetime.now() - timedelta(seconds=1)})
        self.assertEqual(self.Ticket._cron_release_held_notifications(), 0, "still in Spam when the window ends")
        self.assertEqual(ticket.notify_state, "suppressed")
        self._assert_silent(watch, "nothing for a ticket in the Spam stage")

    def _spam_from_both_routes_mails_nobody(self, hold, subject):
        self._hold(hold)
        watch = NotificationWatch(self)
        created = self._new_ticket("Form or API spam %s" % hold, stage="spam")
        self.assertEqual(created.notify_state, "suppressed")
        self._ingest(raw_message("support@hams.com", subject, headers=SES_PASS_HEADERS + BULK_HEADERS))
        mailed = self.Ticket.search([("name", "=", subject)], order="id desc", limit=1)
        self.assertEqual((mailed.stage, mailed.notify_state), ("spam", "suppressed"))
        self._assert_silent(watch, "spam through either route mails nobody (hold %s)" % hold)

    def test_08_a_spam_ticket_from_each_route_mails_nobody(self):
        # Tests [@ANCHOR: hams_helpdesk:COMM_triage_notification_state_on_create]
        # Routes: a form or API create into the Spam stage (both end in create()), and inbound mail the rules flag; with no
        # hold window and with one.
        self._spam_from_both_routes_mails_nobody(0, REAL_MISSED_SPAM_NAMES[0])
        self._spam_from_both_routes_mails_nobody(20, REAL_MISSED_SPAM_NAMES[1])

    def test_09_a_real_inbound_mail_is_held_and_then_sent_after_the_window(self):
        # Tests [@ANCHOR: hams_helpdesk:COMM_triage_notification_state_on_create]
        # Tests [@ANCHOR: hams_helpdesk:cron_release_held_notifications]
        self._hold(20)
        watch = NotificationWatch(self)
        self._ingest(raw_message("support@hams.com", "Antenna analyzer reads high SWR", from_addr="ops@example.org", html=False, body="Reads 3:1 on 20m."))
        ticket = self.Ticket.search([("name", "=", "Antenna analyzer reads high SWR")], limit=1)
        self.assertEqual((ticket.stage, ticket.notify_state), ("new", "held"))
        self._assert_silent(watch, "the inbound message itself is not mailed to the assignee during the window")
        ticket.write({"notify_release_at": fields.Datetime.now() - timedelta(seconds=1)})
        self.Ticket._cron_release_held_notifications()
        self.assertTrue(any(kind == "simple_notification" for _target, kind in watch.bus))

    def test_10_moving_a_ticket_out_of_spam_sends_the_notification_unless_the_setting_says_not(self):
        # Tests [@ANCHOR: hams_helpdesk:triage_notify_on_leaving_spam]
        # Tests [@ANCHOR: hams_helpdesk:triage_release_notification]
        self.assertTrue(self.Ticket._triage_notify_on_leaving_spam(), "on by default (Bruce's to-do text)")
        sent = self._new_ticket("Recovered false positive", stage="spam")
        quiet = self._new_ticket("Recovered, setting off", stage="spam")
        watch = NotificationWatch(self)
        sent.write({"stage": "new"})
        self.assertEqual(sent.notify_state, "sent")
        self.assertTrue(any(kind == "simple_notification" for _target, kind in watch.bus), "the held-back notice goes out now")
        self.settings["hams_helpdesk.triage_notify_on_leaving_spam"] = "0"
        self.assertFalse(self.Ticket._triage_notify_on_leaving_spam())
        before = len(watch.bus)
        quiet.write({"stage": "new"})
        self.assertEqual(len(watch.bus), before, "with the setting off, recovering a ticket sends nothing")
        self.assertEqual(quiet.notify_state, "suppressed")

    def test_11_a_late_spam_verdict_stops_further_mail_about_a_ticket_already_announced(self):
        # Tests [@ANCHOR: hams_helpdesk:mcp_record_verdict]
        ticket = self._new_ticket("Announced, then judged spam")
        self.assertIn(self.on_duty.partner_id, ticket.message_partner_ids)
        ticket.with_user(self.agent_uid).mcp_record_verdict("spam", "Late verdict")
        self.assertFalse(ticket.message_partner_ids, "nobody follows a quarantined ticket")
        watch = NotificationWatch(self)
        ticket.message_post(body="<p>More mail from the sender</p>", message_type="comment", subtype_xmlid="mail.mt_comment")
        self._assert_silent(watch, "later messages on a spam ticket are not mailed")

    def test_12_tickets_the_agent_cannot_judge_are_never_held_and_a_mixed_batch_keeps_its_order(self):
        # Tests [@ANCHOR: hams_helpdesk:COMM_triage_notification_state_on_create]
        self._hold(20)
        exception_ticket = self._new_ticket("A server exception", ticket_type="server_exception")
        self.assertEqual(exception_ticket.notify_state, "sent", "a type the agent cannot see is never delayed")
        batch = self.Ticket.create(
            [
                {"name": "batch held", "description": "<p>x</p>", "ticket_type": "general"},
                {"name": "batch spam", "description": "<p>x</p>", "ticket_type": "general", "stage": "spam"},
                {"name": "batch relay", "description": "<p>x</p>", "ticket_type": "hams_local_relay"},
            ]
        )
        self.assertEqual(batch.mapped("name"), ["batch held", "batch spam", "batch relay"])
        self.assertEqual(batch.mapped("notify_state"), ["held", "suppressed", "held"])

    def test_13_no_caller_can_set_the_notification_state(self):
        # Tests [@ANCHOR: hams_helpdesk:COMM_triage_notification_state_on_create]
        forged = self.Ticket.create(
            {"name": "Forged state", "description": "<p>x</p>", "notify_state": "suppressed", "triage_verdict": "real"}
        )
        self.assertEqual(forged.notify_state, "sent")
