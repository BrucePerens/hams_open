# Copyright © Bruce Perens K6BP.
# SPDX-License-Identifier: AGPL-3.0-or-later

# -*- coding: utf-8 -*-
import base64
from datetime import timedelta

from odoo import fields
from odoo.tests.common import tagged
from odoo.addons.zero_sudo.tests.common import HamsTransactionCase
from odoo.addons.hams_helpdesk.tests.bulk_mail_fixtures import (
    BULK_HEADERS,
    REAL_MISSED_SPAM_NAMES,
    SES_PASS_HEADERS,
    NotificationWatch,
    newsletter_body,
    raw_message,
)


@tagged("post_install", "-at_install", "standard")
class TestMailIngestIncident(HamsTransactionCase):
    """info@hams.com and postmaster@hams.com now route to pager_duty
    (pager.incident) as their canonical record, not directly to
    hams_helpdesk.ticket -- per Bruce's own direction (see
    docs/proposals/EMAIL_SEND_RECEIVE.md and hooks.py's info@ claim /
    data/mail_alias_data.xml's postmaster@ record). Verified end to end
    through the real SES-to-S3-to-Odoo ingest RPC entrypoint
    (hams_helpdesk.ticket.ingest_inbound_email() -- alias-model-agnostic
    underneath, since it calls mail.thread.message_process() directly, so
    reusing it here for pager.incident's own routing is the same real code
    path production traffic uses, not a parallel mock).

    Real, pre-existing behavior discovered while verifying this (not a new
    assumption): `pager.incident` already has its own create() override
    (models/incident_ticket_adapter.py, `_inherit = "pager.incident"`) that
    automatically mirrors every new incident into a linked
    hams_helpdesk.ticket via `action_generate_helpdesk_ticket()`, recording
    the link back on `helpdesk_ticket_id`/`helpdesk_ticket_model`. So a
    hams_helpdesk.ticket DOES get created too -- as a derived mirror of the
    canonical pager.incident, not as an independent, competing route the
    way the old hams_helpdesk-owned info@ alias used to be."""

    @classmethod
    def setUpClass(cls):
        super().setUpClass()
        cls.company = cls.env.ref("base.main_company")
        # Production configures hams.com's alias domain via the website
        # settings UI, not a repo data file -- a fresh test DB has none, so
        # message_process()'s alias-domain match would silently fail to
        # resolve info@/postmaster@ without one.
        alias_domain = cls.env["mail.alias.domain"].search(
            [("name", "=", "hams.com")], limit=1
        )
        if not alias_domain:
            alias_domain = cls.env["mail.alias.domain"].create({"name": "hams.com"})
        if cls.company.alias_domain_id != alias_domain:
            cls.company.alias_domain_id = alias_domain

        cls.ingest_user = cls.env.ref("hams_helpdesk.user_mail_ingest_service")

    def _raw_email(self, to_addr, subject="Test inquiry", from_addr="member@example.com"):
        return (
            f"From: {from_addr}\r\n"
            f"To: {to_addr}\r\n"
            f"Subject: {subject}\r\n"
            "Message-ID: <test-ingest-{}@example.com>\r\n"
            "Content-Type: text/plain; charset=utf-8\r\n"
            "\r\n"
            "This is a test inbound message body.\r\n"
        ).format(subject.replace(" ", "-")).encode("utf-8")

    def test_info_email_creates_pager_incident_with_linked_helpdesk_ticket(self):
        # Tests [@ANCHOR: pager_incident_message_new]
        # A new-thread inbound email routes through message_process() ->
        # message_route() -> message_new() -- the real code path this
        # anchor documents, not a mock of it.
        raw = self._raw_email("info@hams.com", subject="General question about membership")

        incidents_before = self.env["pager.incident"].search_count([])

        self.env["hams_helpdesk.ticket"].with_user(self.ingest_user).ingest_inbound_email(
            base64.b64encode(raw).decode("ascii")
        )

        self.assertEqual(
            self.env["pager.incident"].search_count([]),
            incidents_before + 1,
            "message_process() should have created exactly one new pager.incident via the info@ alias.",
        )

        incident = self.env["pager.incident"].search([], order="id desc", limit=1)
        self.assertIn("General question about membership", incident.name or "")
        self.assertIn("member@example.com", incident.source)
        self.assertEqual(incident.severity, "low")

        # The pre-existing incident_ticket_adapter.py mirror -- see this
        # class's own docstring. Confirms info@ is routed through the real
        # pager.incident model (not a parallel/competing path), not that
        # no helpdesk ticket exists at all.
        self.assertTrue(incident.helpdesk_ticket_id, "Expected the pre-existing helpdesk-ticket mirror to fire.")
        self.assertEqual(incident.helpdesk_ticket_model, "hams_helpdesk.ticket")
        linked_ticket = self.env["hams_helpdesk.ticket"].browse(incident.helpdesk_ticket_id)
        self.assertTrue(linked_ticket.exists())

    def test_info_email_with_on_duty_admin_posts_assignment_note_as_request_user(self):
        # Tests [@ANCHOR: hams_helpdesk:COMM_automated_routing_and_notification]
        """The exact production path of the 2026-09-27 incident (hams1,
        12 AccessErrors on every info@hams.com mail): the ingest daemon's
        RPC call -> info@ -> pager.incident.message_new() -> helpdesk
        ticket create() -> _automated_routing_and_notification() ->
        message_post(partner_ids=[on-duty admin's partner]) -> Odoo core's
        Many2many.write_real() re-checking res.partner read as
        env.transaction.default_env.uid, the RPC caller. See
        hams_helpdesk/security/mail_ingest_security.xml for the mechanism
        and hams_helpdesk/tests/test_mail_ingest.py's test_06 for the
        module-local variant. The on-duty admin is a real is_pager_duty
        shift (get_current_on_duty_admin()), and the transaction's default
        env is set to the ingest account exactly as odoo/service/model.py
        does for every RPC call -- nothing here is mocked.
        """
        on_duty = self.env["res.users"].create(
            {
                "name": "On Duty Admin For Ingest Test",
                "login": "on_duty_admin_ingest_test",
                "group_ids": [
                    (
                        6,
                        0,
                        [
                            self.env.ref("pager_duty.group_pager_admin").id,
                            self.env.ref("hams_helpdesk.group_helpdesk_manager").id,
                        ],
                    )
                ],
            }
        )
        now = fields.Datetime.now()
        self.env["calendar.event"].create(
            {
                "name": "Ingest regression shift",
                "start": now - timedelta(hours=1),
                "stop": now + timedelta(hours=1),
                "is_pager_duty": True,
                "user_id": on_duty.id,
            }
        )

        ingest_env = self.env(user=self.ingest_user.id)
        transaction = self.env.transaction
        previous_default_env = transaction.default_env
        transaction.default_env = ingest_env
        self.addCleanup(setattr, transaction, "default_env", previous_default_env)

        raw = self._raw_email("info@hams.com", subject="Needs an on-duty reply")
        ingest_env["hams_helpdesk.ticket"].ingest_inbound_email(
            base64.b64encode(raw).decode("ascii")
        )

        incident = self.env["pager.incident"].search(
            [("name", "ilike", "Needs an on-duty reply")], limit=1
        )
        self.assertTrue(incident, "Expected the info@ email to create a pager.incident.")
        ticket = self.env["hams_helpdesk.ticket"].browse(incident.helpdesk_ticket_id)
        self.assertTrue(ticket.exists(), "Expected the linked helpdesk ticket.")
        self.assertEqual(ticket.user_id, on_duty)
        assignment_note = ticket.message_ids.filtered(
            lambda message: on_duty.partner_id in message.partner_ids
        )
        self.assertTrue(
            assignment_note,
            "The on-duty assignment note must have been posted -- in production "
            "this exact message_post(partner_ids=...) raised the AccessError.",
        )

    def test_postmaster_email_creates_pager_incident(self):
        raw = self._raw_email(
            "postmaster@hams.com",
            subject="Delivery problem report",
            from_addr="other-admin@example.net",
        )

        incidents_before = self.env["pager.incident"].search_count([])

        self.env["hams_helpdesk.ticket"].with_user(self.ingest_user).ingest_inbound_email(
            base64.b64encode(raw).decode("ascii")
        )

        self.assertEqual(
            self.env["pager.incident"].search_count([]),
            incidents_before + 1,
            "message_process() should have created exactly one new pager.incident via the postmaster@ alias.",
        )
        incident = self.env["pager.incident"].search([], order="id desc", limit=1)
        self.assertIn("other-admin@example.net", incident.source)
        self.assertEqual(incident.severity, "medium")
        # "medium" is also skipped by create()'s severity gate, so this
        # only holds because message_new() requests the ticket explicitly.
        self.assertTrue(incident.helpdesk_ticket_id, "A postmaster@ email must still produce a helpdesk ticket.")
        self.assertEqual(incident.helpdesk_ticket_model, "hams_helpdesk.ticket")

    # Whether a vacation/unsubscribe/bounce message to postmaster@ gets
    # dropped before it ever reaches this alias is hams_base's own
    # mail.thread override's job (models/mail_thread.py) -- pager_duty
    # doesn't depend on hams_base, so that filtering isn't guaranteed to be
    # installed here. See hams_base/tests/test_mail_thread.py for that
    # coverage instead.


    # ------------------------------------------------------------------
    # Bulk / marketing mail through info@ -- the route every production miss of 2026-10-28..11-03 came through.
    # ------------------------------------------------------------------

    def _on_duty_admin(self):
        user = self.env["res.users"].create(
            {
                "name": "Bulk Test On Duty",
                "login": "bulk_test_on_duty",
                "email": "bulk-test-on-duty@example.org",
                "group_ids": [(6, 0, [self.env.ref("pager_duty.group_pager_admin").id, self.env.ref("hams_helpdesk.group_helpdesk_manager").id])],
            }
        )
        now = fields.Datetime.now()
        self.env["calendar.event"].create(
            {"name": "Bulk test shift", "start": now - timedelta(hours=1), "stop": now + timedelta(hours=1), "is_pager_duty": True, "user_id": user.id}
        )
        return user

    def _ingest_raw(self, raw):
        self.env["hams_helpdesk.ticket"].with_user(self.ingest_user).ingest_inbound_email(base64.b64encode(raw).decode("ascii"))

    def test_info_bulk_newsletter_is_quarantined_resolved_and_mails_nobody(self):
        # Tests [@ANCHOR: pager_incident_message_new]
        # Tests [@ANCHOR: hams_helpdesk:inbound_spam_reasons]
        # The mail has no subject pattern: only its headers and HTML shape say what it is.
        on_duty = self._on_duty_admin()
        watch = NotificationWatch(self)
        subject = REAL_MISSED_SPAM_NAMES[1]
        self._ingest_raw(raw_message("info@hams.com", subject, headers=SES_PASS_HEADERS + BULK_HEADERS))
        incident = self.env["pager.incident"].search([("name", "=", subject)], order="id desc", limit=1)
        self.assertTrue(incident)
        ticket = self.env["hams_helpdesk.ticket"].browse(incident.helpdesk_ticket_id)
        self.assertEqual(ticket.name, "[PAGER] " + subject)
        self.assertEqual(ticket.stage, "spam", "the mirrored ticket is in the visible quarantine, not deleted")
        self.assertEqual(ticket.user_id, on_duty)
        self.assertEqual(incident.status, "resolved", "a quarantined message is not an open incident on the pager board")
        self.assertTrue(incident.time_resolved)
        self.assertIn("List-Unsubscribe", ticket.description, "the ticket records the reasons")
        self.assertFalse(
            self.env["calendar.event"].search([("name", "ilike", subject)]), "no Incident Response calendar block for junk"
        )
        self.assertEqual(watch.mails().ids, [], "no email")
        self.assertEqual(watch.notifications().ids, [], "no notification row")
        self.assertEqual(watch.bus, [], "no bus message: no toast and no board refresh")

    def test_info_ordinary_mail_still_notifies_and_stays_open(self):
        # The control for the test above: the same harness sees the notice for a real message.
        self._on_duty_admin()
        watch = NotificationWatch(self)
        self._ingest_raw(raw_message("info@hams.com", "Question about the club directory", from_addr="visitor@example.com", html=False, headers=SES_PASS_HEADERS))
        incident = self.env["pager.incident"].search([("name", "=", "Question about the club directory")], limit=1)
        self.assertEqual(incident.status, "open")
        self.assertEqual(self.env["hams_helpdesk.ticket"].browse(incident.helpdesk_ticket_id).stage, "new")
        self.assertGreater(watch.count(), 0)
        self.assertTrue(any(kind == "simple_notification" for _target, kind in watch.bus))

    def test_info_mail_from_a_member_is_not_flagged_and_a_ses_fail_still_is(self):
        member = self.env["res.users"].create({"name": "Member Sender", "login": "member_sender", "email": "member-sender@example.org"})
        subject = REAL_MISSED_SPAM_NAMES[4]
        self._ingest_raw(raw_message("info@hams.com", subject, from_addr="member-sender@example.org", headers=BULK_HEADERS))
        incident = self.env["pager.incident"].search([("name", "=", subject)], limit=1)
        self.assertEqual(self.env["hams_helpdesk.ticket"].browse(incident.helpdesk_ticket_id).stage, "new")
        self.assertEqual(incident.status, "open")
        self.assertTrue(member.partner_id)
        # The SES verdict reaches the pager route too (it only reached the admin@/support@ route before).
        self._ingest_raw(
            raw_message("info@hams.com", "Plain question", from_addr="visitor@example.com", html=False, headers="X-SES-Spam-Verdict: FAIL\r\nX-SES-Virus-Verdict: PASS\r\n")
        )
        incident = self.env["pager.incident"].search([("name", "=", "Plain question")], limit=1)
        self.assertEqual(self.env["hams_helpdesk.ticket"].browse(incident.helpdesk_ticket_id).stage, "spam")

    def test_monitoring_incidents_are_never_run_through_the_mail_filter(self):
        # Tests [@ANCHOR: pd_helpdesk_adapter]
        # "DX Spot Feed Freshness" and the other monitoring names must stay in "new" whatever their description looks like:
        # the filter applies to inbound mail sources only.
        for name in ("DX Spot Feed Freshness", "Backup Manager: hams_prod_pgbackrest_b2", "INC-AUTO"):
            incident = self.env["pager.incident"].create(
                {"name": name, "source": name, "severity": "high", "description": newsletter_body()}
            )
            incident.action_generate_helpdesk_ticket()
            ticket = self.env["hams_helpdesk.ticket"].browse(incident.helpdesk_ticket_id)
            self.assertEqual(ticket.stage, "new", name)
            self.assertEqual(incident.status, "open", name)
