# This software is distributed under the terms of the Affero General Public License (AGPL-3).

# -*- coding: utf-8 -*-
import base64
from datetime import timedelta

from odoo import fields
from odoo.tests.common import tagged
from odoo.addons.zero_sudo.tests.common import HamsTransactionCase


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
