# This software is distributed under the terms of the Affero General Public License (AGPL-3).

# -*- coding: utf-8 -*-
import base64

from odoo.exceptions import AccessError
from odoo.tests.common import tagged
from odoo.addons.zero_sudo.tests.common import HamsTransactionCase


@tagged("post_install", "-at_install", "standard")
class TestMailIngest(HamsTransactionCase):
    """Verifies EMAIL_SEND_RECEIVE.md item 2: the SES-to-S3-to-Odoo inbound
    mail daemon's RPC entrypoint. This is the real answer to the exact
    permission question a prior pass left open -- run against a live test
    DB rather than assumed, so an AccessError here means the ir.model.access
    grant on ``mail_ingest_security.xml``'s service group is genuinely too
    narrow, not a guess.
    """

    @classmethod
    def setUpClass(cls):
        super().setUpClass()
        cls.company = cls.env.ref("base.main_company")
        # Production configures hams.com's alias domain via the website
        # settings UI, not a repo data file (see EMAIL_SEND_RECEIVE.md) --
        # a fresh test DB has none, so message_process()'s alias-domain
        # match would silently fail to resolve support@/info@ without one.
        alias_domain = cls.env["mail.alias.domain"].search(
            [("name", "=", "hams.com")], limit=1
        )
        if not alias_domain:
            alias_domain = cls.env["mail.alias.domain"].create({"name": "hams.com"})
        if cls.company.alias_domain_id != alias_domain:
            cls.company.alias_domain_id = alias_domain

        cls.ingest_user = cls.env.ref("hams_helpdesk.user_mail_ingest_service")

    def _raw_email(self, to_addr, subject="Test inquiry", from_addr="customer@example.com"):
        return (
            f"From: {from_addr}\r\n"
            f"To: {to_addr}\r\n"
            f"Subject: {subject}\r\n"
            "Message-ID: <test-ingest-{}@example.com>\r\n"
            "Content-Type: text/plain; charset=utf-8\r\n"
            "\r\n"
            "This is a test inbound support request body.\r\n"
        ).format(subject.replace(" ", "-")).encode("utf-8")

    def test_01_service_account_ingests_support_email_into_ticket(self):
        # Tests [@ANCHOR: COMM_helpdesk_mail_ingest]
        raw = self._raw_email("support@hams.com", subject="Radio will not power on")

        tickets_before = self.env["hams_helpdesk.ticket"].search_count([])
        self.env["hams_helpdesk.ticket"].with_user(self.ingest_user).ingest_inbound_email(
            base64.b64encode(raw).decode("ascii")
        )
        tickets_after = self.env["hams_helpdesk.ticket"].search([], order="id desc", limit=1)

        self.assertEqual(
            self.env["hams_helpdesk.ticket"].search_count([]),
            tickets_before + 1,
            "message_process() should have created exactly one new ticket via the support@ alias.",
        )
        self.assertIn("Radio will not power on", tickets_after.name or "")

    def test_02_service_account_ingests_info_email_into_ticket(self):
        raw = self._raw_email("info@hams.com", subject="General question about membership")

        tickets_before = self.env["hams_helpdesk.ticket"].search_count([])
        self.env["hams_helpdesk.ticket"].with_user(self.ingest_user).ingest_inbound_email(
            base64.b64encode(raw).decode("ascii")
        )

        self.assertEqual(
            self.env["hams_helpdesk.ticket"].search_count([]),
            tickets_before + 1,
            "message_process() should have created exactly one new ticket via the info@ alias.",
        )

    def test_03_non_service_account_is_denied(self):
        """The real access boundary is the explicit login check inside
        ingest_inbound_email(), not ir.model.access -- prove it actually
        holds for an ordinary internal user, not just the intended caller.
        """
        other_user = self.env["res.users"].create(
            {
                "name": "Not The Ingest Service",
                "login": "not_mail_ingest_test",
                "group_ids": [
                    (6, 0, [self.env.ref("hams_helpdesk.group_helpdesk_manager").id])
                ],
            }
        )
        raw = self._raw_email("support@hams.com")
        with self.assertRaises(AccessError):
            self.env["hams_helpdesk.ticket"].with_user(other_user).ingest_inbound_email(
                base64.b64encode(raw).decode("ascii")
            )
