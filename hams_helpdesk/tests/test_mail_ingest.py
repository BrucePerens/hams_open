# Copyright © Bruce Perens K6BP.
# SPDX-License-Identifier: AGPL-3.0-or-later

# -*- coding: utf-8 -*-
import ast
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

    # info@hams.com used to create a hams_helpdesk.ticket here; per Bruce's
    # own direction it now routes to pager_duty (pager.incident) instead --
    # see pager_duty/tests/test_hooks.py for the alias claim and
    # pager_duty/tests/test_mail_ingest_incident.py for the end-to-end
    # ingest test. hams_helpdesk doesn't depend on pager_duty, so it can't
    # verify info@'s real routing target from here.

    def test_04_ingest_links_existing_customer_partner(self):
        # Tests [@ANCHOR: COMM_helpdesk_message_new]
        # Bug-hunt regression (2026-09-09): before message_new() was
        # overridden, an email-ingested ticket's partner_id was always
        # False even when the sender's address matched an existing
        # customer -- message_route() had already resolved the sender to
        # a real partner (msg_dict['author_id']), but mail.thread's own
        # default message_new() never used it, since hams_helpdesk.ticket
        # has no email_from-style field for _mail_get_primary_email_field()
        # to auto-populate. That silently orphaned every emailed-in ticket
        # from the customer.partner_id-based portal listing
        # (/my/tickets), the write() stage-change mail-back, and the
        # automated "ensure customer is subscribed" step.
        partner = self.env["res.partner"].create(
            {"name": "Known Customer", "email": "known.customer@example.com"}
        )
        raw = self._raw_email(
            "support@hams.com",
            subject="Antenna tuner not tuning",
            from_addr="known.customer@example.com",
        )
        self.env["hams_helpdesk.ticket"].with_user(self.ingest_user).ingest_inbound_email(
            base64.b64encode(raw).decode("ascii")
        )
        ticket = self.env["hams_helpdesk.ticket"].search(
            [("name", "ilike", "Antenna tuner not tuning")], limit=1
        )
        self.assertTrue(ticket, "Expected the inbound email to create a ticket.")
        self.assertEqual(
            ticket.partner_id,
            partner,
            "An inbound email from a known customer's address must link the "
            "resulting ticket to that customer's partner_id, or the customer "
            "can never see their own ticket under /my/tickets.",
        )

    def test_05_ingest_with_unknown_sender_leaves_partner_unset(self):
        # Tests [@ANCHOR: COMM_helpdesk_message_new]
        # The IF/unwanted-behavior branch: an address matching no existing
        # partner must not raise and must not fabricate a partner_id link.
        raw = self._raw_email(
            "support@hams.com",
            subject="Totally unknown sender inquiry",
            from_addr="nobody-on-file@example.com",
        )
        self.env["hams_helpdesk.ticket"].with_user(self.ingest_user).ingest_inbound_email(
            base64.b64encode(raw).decode("ascii")
        )
        ticket = self.env["hams_helpdesk.ticket"].search(
            [("name", "ilike", "Totally unknown sender inquiry")], limit=1
        )
        self.assertTrue(ticket, "Expected the inbound email to create a ticket.")
        self.assertFalse(
            ticket.partner_id,
            "An unrecognized sender must not be linked to an arbitrary partner_id.",
        )

    def test_06_partner_ids_check_runs_as_the_request_user(self):
        # Tests [@ANCHOR: hams_helpdesk:COMM_automated_routing_and_notification]
        """Regression for the 2026-09-27 production incident: every inbound
        mail to info@hams.com died with "Failed to write field
        mail.message.partner_ids / not allowed to access res.partner",
        reported against the ingest service account's own uid even though
        the failing message_post() ran under user_helpdesk_service via
        zero_sudo's with_user() elevation. security/mail_ingest_security.xml
        carries the full mechanism; in short, Odoo 19's
        Many2many.write_real() -> _check_sudo_commands() re-runs the
        res.partner read check as env.transaction.default_env.uid -- the
        account that made the RPC call -- and discards every with_user()
        applied since.

        Every other test in this class, and any `odoo shell`, silently
        misses that because their transaction's default env is the
        superuser. The real RPC dispatcher (odoo/service/model.py:
        `env.transaction.default_env = env`) and HTTP layer (odoo/http.py)
        set it to the CALLER's env, so this test does exactly the same for
        the ingest account before calling in -- the production execution
        path, not a mock of it.
        """
        manager = self.env["res.users"].create(
            {
                "name": "Assignee For Ingest Test",
                "login": "ingest_assignee_test",
                "group_ids": [
                    (6, 0, [self.env.ref("hams_helpdesk.group_helpdesk_manager").id])
                ],
            }
        )
        # Production tickets get their assignee from pager_duty's on-duty
        # admin; hams_helpdesk on its own has no on-duty source (its
        # calendar_event.get_current_on_duty_admin() returns False), so hand
        # create() an assignee the same way it would otherwise receive one,
        # through vals["user_id"] -- via the alias's real alias_defaults --
        # so _automated_routing_and_notification() takes the
        # message_post(partner_ids=...) branch that failed in production.
        alias = self.env["mail.alias"].search([("alias_name", "=", "support")], limit=1)
        self.assertTrue(alias, "hams_helpdesk's own support@ alias must exist.")
        defaults = ast.literal_eval(alias.alias_defaults or "{}")
        defaults["user_id"] = manager.id
        alias.alias_defaults = repr(defaults)

        ingest_env = self.env(user=self.ingest_user.id)
        transaction = self.env.transaction
        previous_default_env = transaction.default_env
        transaction.default_env = ingest_env
        self.addCleanup(setattr, transaction, "default_env", previous_default_env)

        self.assertTrue(
            ingest_env["res.partner"].has_access("read"),
            "group_mail_ingest_service needs read on res.partner: Odoo core "
            "checks mail.message.partner_ids as the request user, not as the "
            "elevated helpdesk service account (see mail_ingest_security.xml).",
        )

        raw = self._raw_email("support@hams.com", subject="Assigned on arrival")
        ingest_env["hams_helpdesk.ticket"].ingest_inbound_email(
            base64.b64encode(raw).decode("ascii")
        )

        ticket = self.env["hams_helpdesk.ticket"].search(
            [("name", "ilike", "Assigned on arrival")], limit=1
        )
        self.assertTrue(ticket, "Expected the inbound email to create a ticket.")
        self.assertEqual(ticket.user_id, manager)
        assignment_note = ticket.message_ids.filtered(
            lambda message: manager.partner_id in message.partner_ids
        )
        self.assertTrue(
            assignment_note,
            "The assignment note (message_post with partner_ids) must have been "
            "posted; in production this is exactly the write that raised.",
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

    # ------------------------------------------------------------------
    # Mail-ingestion spam filter on the direct admin@/support@ route
    # ------------------------------------------------------------------

    def _ingest(self, raw):
        self.env["hams_helpdesk.ticket"].with_user(self.ingest_user).ingest_inbound_email(
            base64.b64encode(raw).decode("ascii")
        )

    def _spam_notes(self, ticket):
        return ticket.message_ids.filtered(
            lambda m: "Possible spam/phishing" in (m.body or "")
        )

    def test_07_spam_subject_to_support_is_quarantined_not_dropped(self):
        # Tests [@ANCHOR: hams_helpdesk:COMM_helpdesk_message_new_spam_filter]
        # Production ticket #43 ("Action Required: 5 Pending Violation
        # Reports") reached support@/admin@ directly and sat in "new",
        # because the filter used to run only in pager_duty's adapter.
        raw = self._raw_email(
            "support@hams.com",
            subject="Action Required: 5 Pending Violation Reports",
            from_addr="compliance-desk@example.net",
        )
        tickets_before = self.env["hams_helpdesk.ticket"].search_count([])
        self._ingest(raw)
        self.assertEqual(
            self.env["hams_helpdesk.ticket"].search_count([]),
            tickets_before + 1,
            "A flagged message must still become a ticket, never be dropped.",
        )
        ticket = self.env["hams_helpdesk.ticket"].search(
            [("name", "ilike", "Pending Violation Reports")], order="id desc", limit=1
        )
        self.assertTrue(ticket)
        self.assertEqual(ticket.stage, "spam")
        notes = self._spam_notes(ticket)
        self.assertTrue(notes, "The ticket must record why it was quarantined.")
        self.assertIn("compliance/violation scare spam", notes[0].body)

    def test_08_brand_impersonation_link_in_body_is_quarantined(self):
        # Tests [@ANCHOR: hams_helpdesk:COMM_helpdesk_message_new_spam_filter]
        # The body-based signal (msg_dict["body"]), not just the subject:
        # a neutral subject with a QuickBooks-branded body whose link goes
        # to a non-Intuit domain, the shape of phishing ticket #13.
        raw = (
            "From: billing-alerts@example.org\r\n"
            "To: admin@hams.com\r\n"
            "Subject: Your account notice\r\n"
            "Message-ID: <test-ingest-qbo-phish@example.org>\r\n"
            "Content-Type: text/html; charset=utf-8\r\n"
            "\r\n"
            "<p>QuickBooks: a customer left a review. "
            '<a href="https://rwiwanksiit.vu/qbo/">View it here</a></p>\r\n'
        ).encode("utf-8")
        self._ingest(raw)
        ticket = self.env["hams_helpdesk.ticket"].search(
            [("name", "=", "Your account notice")], order="id desc", limit=1
        )
        self.assertTrue(ticket)
        self.assertEqual(ticket.stage, "spam")
        notes = self._spam_notes(ticket)
        self.assertTrue(notes)
        self.assertIn("quickbooks impersonation", notes[0].body)

    def test_09_ordinary_support_request_stays_in_new(self):
        # Tests [@ANCHOR: hams_helpdesk:COMM_helpdesk_message_new_spam_filter]
        # The false-positive side: a plain support request is not touched.
        raw = self._raw_email(
            "support@hams.com", subject="Cannot log my QSOs from the shack page"
        )
        self._ingest(raw)
        ticket = self.env["hams_helpdesk.ticket"].search(
            [("name", "ilike", "Cannot log my QSOs")], order="id desc", limit=1
        )
        self.assertTrue(ticket)
        self.assertEqual(ticket.stage, "new")
        self.assertFalse(self._spam_notes(ticket))
