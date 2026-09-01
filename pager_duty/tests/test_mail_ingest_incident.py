# This software is distributed under the terms of the Affero General Public License (AGPL-3).

# -*- coding: utf-8 -*-
import base64

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

    # Whether a vacation/unsubscribe/bounce message to postmaster@ gets
    # dropped before it ever reaches this alias is hams_base's own
    # mail.thread override's job (models/mail_thread.py) -- pager_duty
    # doesn't depend on hams_base, so that filtering isn't guaranteed to be
    # installed here. See hams_base/tests/test_mail_thread.py for that
    # coverage instead.
