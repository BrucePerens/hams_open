# Copyright © Bruce Perens K6BP.
# SPDX-License-Identifier: AGPL-3.0-or-later
# -*- coding: utf-8 -*-
"""The triage verdict recorded on every ticket (Bruce, 2026-11-02, night_shift_todo ticket-triage-agent-reviews-tickets-
on-submission-and-marks-spam): the agent's and the inbound rules' verdict with reasons, the quarantine verdicts as a
setting, and a person's move out of the Spam stage as the override and the labelled example."""
import base64

from odoo.exceptions import AccessError, UserError
from odoo.tests.common import tagged
from odoo.addons.zero_sudo.tests.common import HamsTransactionCase

from .bulk_mail_fixtures import REAL_MISSED_SPAM_NAMES, SES_PASS_HEADERS, BULK_HEADERS, raw_message


@tagged("post_install", "-at_install", "standard")
class TestTriageVerdict(HamsTransactionCase):
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
        self.agent_uid = self.env["zero_sudo.security.utils"]._get_service_uid("hams_helpdesk.user_ai_triage_service")
        self.Ticket = self.env["hams_helpdesk.ticket"]

    def _ticket(self, name="Triage verdict test"):
        return self.Ticket.with_user(self.env.ref("base.user_admin")).create(
            {"name": name, "description": "<p>test</p>", "ticket_type": "general"}
        )

    def _agent(self, ticket):
        return ticket.with_user(self.agent_uid)

    def _set_quarantine(self, value):
        """Sets the setting for this test. The real reader is a cached zero_sudo read of a system parameter, so the test
        answers that one key itself (test_09 proves the real key is readable and has the documented default)."""
        utils_model = type(self.env["zero_sudo.security.utils"])
        real = utils_model._get_system_param

        def answer(utils, key, default=None):
            if key == "hams_helpdesk.triage_quarantine_verdicts":
                return value
            return real(utils, key, default)

        self.safe_patch_object(utils_model, "_get_system_param", answer, create=True)

    def test_01_a_spam_verdict_is_recorded_with_its_reasons_and_quarantines(self):
        # Tests [@ANCHOR: hams_helpdesk:mcp_record_verdict]
        # Tests [@ANCHOR: hams_helpdesk:triage_record_verdict]
        # Tests [@ANCHOR: hams_helpdesk:triage_quarantine_verdicts]
        ticket = self._ticket()
        self._agent(ticket).mcp_record_verdict("spam", "Supplement newsletter; sender and request do not match")
        self.assertEqual(ticket.stage, "spam")
        self.assertEqual(ticket.triage_verdict, "spam")
        self.assertEqual(ticket.triage_verdict_source, "agent")
        self.assertIn("Supplement newsletter", ticket.triage_reasons)
        self.assertTrue(ticket.triage_verdict_at)
        self.assertFalse(ticket.triage_overridden)

    def test_02_real_and_unsure_verdicts_are_recorded_and_leave_the_ticket_alone(self):
        # Tests [@ANCHOR: hams_helpdesk:mcp_record_verdict]
        for verdict in ("real", "unsure"):
            ticket = self._ticket("Verdict %s" % verdict)
            self._agent(ticket).mcp_record_verdict(verdict, "A member asks about a rotator")
            self.assertEqual((ticket.stage, ticket.triage_verdict), ("new", verdict))
            note = ticket.message_ids.sorted(key=lambda m: m.id)[-1]
            self.assertIn("AI triage verdict", note.body)
            self.assertTrue(note.subtype_id.internal)

    def test_03_which_verdicts_quarantine_is_a_setting(self):
        # Tests [@ANCHOR: hams_helpdesk:triage_quarantine_verdicts]
        # Tests [@ANCHOR: hams_helpdesk:mcp_record_verdict]
        self.assertEqual(self.Ticket._triage_quarantine_verdicts(), {"spam", "phishing"}, "the default")
        phishing = self._ticket("Phishing by default")
        self._agent(phishing).mcp_record_verdict("phishing", "Fake invoice link")
        self.assertEqual(phishing.stage, "spam")
        self._set_quarantine("spam, nonsense")
        self.assertEqual(self.Ticket._triage_quarantine_verdicts(), {"spam"}, "unknown words are ignored")
        recorded_only = self._ticket("Phishing only recorded")
        self._agent(recorded_only).mcp_record_verdict("phishing", "Fake invoice link")
        self.assertEqual((recorded_only.stage, recorded_only.triage_verdict), ("new", "phishing"))
        self._set_quarantine("")
        self.assertEqual(self.Ticket._triage_quarantine_verdicts(), set(), "an empty setting quarantines nothing")

    def test_04_an_unknown_verdict_and_a_ticket_already_picked_up_are_refused(self):
        # Tests [@ANCHOR: hams_helpdesk:mcp_record_verdict]
        # Tests [@ANCHOR: hams_helpdesk:triage_record_verdict]
        ticket = self._ticket()
        with self.assertRaises(UserError):
            self._agent(ticket).mcp_record_verdict("maybe", "x")
        with self.assertRaises(UserError):
            ticket._triage_record_verdict("maybe", "x", "agent")
        ticket.write({"stage": "in_progress"})
        with self.assertRaises(UserError):
            self._agent(ticket).mcp_record_verdict("real", "x")
        self.assertFalse(ticket.triage_verdict)

    def test_05_a_person_moving_a_ticket_out_of_spam_overrides_it_for_good(self):
        # Tests [@ANCHOR: hams_helpdesk:triage_record_override]
        # Tests [@ANCHOR: hams_helpdesk:mcp_record_verdict]
        ticket = self._ticket()
        self._agent(ticket).mcp_record_verdict("spam", "Looks like a newsletter")
        ticket.write({"stage": "new"})
        self.assertTrue(ticket.triage_overridden)
        self.assertTrue(ticket.triage_overridden_at)
        self.assertEqual(ticket.triage_verdict, "spam", "the verdict stays: with the flag it is the labelled example")
        self.assertEqual(ticket.triage_verdict_source, "agent")
        with self.assertRaises(UserError):
            self._agent(ticket).mcp_record_verdict("spam", "again")
        with self.assertRaises(UserError):
            self._agent(ticket).mcp_mark_spam("again")
        self.assertEqual(ticket.stage, "new")

    def test_06_moving_a_ticket_the_agent_never_judged_records_no_override(self):
        # Tests [@ANCHOR: hams_helpdesk:triage_record_override]
        ticket = self._ticket()
        ticket.write({"stage": "spam"})
        ticket.write({"stage": "new"})
        self.assertFalse(ticket.triage_overridden)
        self.assertFalse(ticket.triage_verdict)

    def test_07_the_inbound_rules_record_their_verdict_and_reasons_and_a_person_can_override_it(self):
        # Tests [@ANCHOR: hams_helpdesk:triage_record_verdict]
        # Tests [@ANCHOR: hams_helpdesk:triage_record_override]
        subject = REAL_MISSED_SPAM_NAMES[0]
        raw = raw_message("support@hams.com", subject, headers=SES_PASS_HEADERS + BULK_HEADERS)
        self.Ticket.with_user(self.env.ref("hams_helpdesk.user_mail_ingest_service")).ingest_inbound_email(
            base64.b64encode(raw).decode("ascii")
        )
        ticket = self.Ticket.search([("name", "=", subject)], order="id desc", limit=1)
        self.assertEqual((ticket.stage, ticket.triage_verdict, ticket.triage_verdict_source), ("spam", "spam", "rules"))
        self.assertIn("List-Unsubscribe", ticket.triage_reasons)
        ticket.write({"stage": "new"})
        self.assertTrue(ticket.triage_overridden, "a person correcting the rules is recorded too")

    def test_08_no_caller_can_set_the_verdict_on_create_or_write(self):
        # Tests [@ANCHOR: hams_helpdesk:triage_record_verdict]
        forged = self.Ticket.create(
            {"name": "Forged verdict", "description": "<p>x</p>", "triage_verdict": "real", "triage_overridden": True}
        )
        self.assertFalse(forged.triage_verdict)
        self.assertFalse(forged.triage_overridden)
        portal = self.env["res.users"].create(
            {
                "name": "Portal triage forger",
                "login": "portal_triage_forger",
                "email": "portal_triage_forger@example.org",
                "group_ids": [(6, 0, [self.env.ref("base.group_portal").id])],
            }
        )
        with self.assertRaises(AccessError):
            forged.with_user(portal).write({"triage_verdict": "real"})

    def test_08b_a_customer_cannot_record_a_verdict_or_post_a_note_on_their_own_ticket(self):
        # Tests [@ANCHOR: hams_helpdesk:mcp_record_verdict]
        # Tests [@ANCHOR: hams_helpdesk:mcp_post_internal_note]
        # Tests [@ANCHOR: hams_helpdesk:require_triage_caller]
        portal = self.env["res.users"].create(
            {
                "name": "Portal verdict caller",
                "login": "portal_verdict_caller",
                "email": "portal_verdict_caller@example.org",
                "group_ids": [(6, 0, [self.env.ref("base.group_portal").id])],
            }
        )
        ticket = self.Ticket.create(
            {"name": "Own ticket", "description": "<p>x</p>", "ticket_type": "general", "partner_id": portal.partner_id.id}
        )
        self.assertTrue(ticket.with_user(portal).read(["stage"]), "the customer can read their own ticket")
        with self.assertRaises(AccessError):
            ticket.with_user(portal).mcp_record_verdict("real", "A genuine request")
        with self.assertRaises(AccessError):
            ticket.with_user(portal).mcp_mark_spam("hide me")
        with self.assertRaises(AccessError):
            ticket.with_user(portal).mcp_post_internal_note("note planted by a customer")
        self.assertFalse(ticket.triage_verdict)
        self.assertEqual(ticket.stage, "new")

    def test_09_the_setting_keys_are_readable_through_zero_sudo_with_the_documented_default(self):
        # Tests [@ANCHOR: hams_helpdesk:triage_quarantine_verdicts]
        utils = self.env["zero_sudo.security.utils"]
        for key in (
            "hams_helpdesk.triage_quarantine_verdicts",
            "hams_helpdesk.triage_notify_hold_seconds",
            "hams_helpdesk.triage_notify_on_leaving_spam",
            "hams_helpdesk.triage_stall_seconds",
        ):
            self.assertIn(key, utils._get_param_read_whitelist())
        self.assertEqual(self.Ticket._triage_quarantine_verdicts(), {"spam", "phishing"})
