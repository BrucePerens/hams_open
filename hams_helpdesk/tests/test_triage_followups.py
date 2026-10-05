# Copyright © Bruce Perens K6BP.
# SPDX-License-Identifier: AGPL-3.0-or-later
# -*- coding: utf-8 -*-
"""hams_helpdesk.ticket.mcp_customer_followups(): the read-only lookup the AI triage agent uses to find a
ticket it already noted whose customer wrote again (decided by the coordinator on Bruce's instruction,
2026-10-05). The daemon half is tested in hams_com daemons/hams_ticket_triage_mcp."""
from datetime import timedelta

from markupsafe import Markup

from odoo import fields
from odoo.tests.common import tagged
from odoo.addons.zero_sudo.tests.common import HamsTransactionCase


@tagged("post_install", "-at_install", "standard")
class TestTriageCustomerFollowups(HamsTransactionCase):
    # Tests [@ANCHOR: hams_helpdesk:mcp_customer_followups]

    def setUp(self):
        super().setUp()
        self.admin = self.env.ref("base.user_admin")
        self.ai_uid = self.env["zero_sudo.security.utils"]._get_service_uid("hams_helpdesk.user_ai_triage_service")
        self.customer = self.env["res.partner"].create({"name": "Follow-up Customer", "email": "fu-customer@example.com"})
        self.customer_user = self.env["res.users"].create({
            "name": "Follow-up Customer",
            "login": "fu_customer_portal",
            "partner_id": self.customer.id,
            "group_ids": [(6, 0, [self.env.ref("base.group_portal").id])],
        })
        self.ticket = self.env["hams_helpdesk.ticket"].with_user(self.admin).create({
            "name": "Follow-up test", "description": "<p>help</p>", "ticket_type": "general",
            "partner_id": self.customer.id,
        })
        self.long_ago = "2000-01-01 00:00:00"

    def _ask(self, cutoffs):
        return self.env["hams_helpdesk.ticket"].with_user(self.ai_uid).mcp_customer_followups(cutoffs)

    def _customer_says(self, body, ticket=None):
        (ticket or self.ticket).with_user(self.admin).message_post(
            body=Markup(body), author_id=self.customer.id, message_type="comment", subtype_xmlid="mail.mt_comment",
        )

    def test_01_a_customer_message_after_the_cutoff_is_reported_as_plain_text(self):
        self._customer_says("<p>It <b>still</b> fails.</p>")
        result = self._ask({str(self.ticket.id): self.long_ago})
        self.assertEqual(list(result), [str(self.ticket.id)])
        self.assertIn("still", result[str(self.ticket.id)][0]["body"])
        self.assertNotIn("<b>", result[str(self.ticket.id)][0]["body"])

    def test_02_nothing_is_reported_when_the_cutoff_is_after_the_message(self):
        self._customer_says("<p>earlier</p>")
        later = fields.Datetime.to_string(fields.Datetime.now() + timedelta(hours=1))
        self.assertEqual(self._ask({str(self.ticket.id): later}), {})

    def test_03_the_agents_own_note_and_staff_replies_are_not_customer_follow_ups(self):
        self.ticket.with_user(self.admin).message_post(body="<p>AI note</p>", subtype_xmlid="mail.mt_note")
        self.ticket.with_user(self.admin).message_post(
            body="<p>Staff reply</p>", author_id=self.admin.partner_id.id, message_type="comment",
            subtype_xmlid="mail.mt_comment",
        )
        self.assertEqual(self._ask({str(self.ticket.id): self.long_ago}), {})

    def test_04_a_spam_stage_ticket_is_never_reported(self):
        self._customer_says("<p>hello?</p>")
        self.ticket.with_user(self.admin).write({"stage": "spam"})
        self.assertEqual(self._ask({str(self.ticket.id): self.long_ago}), {})

    def test_05_a_ticket_outside_the_ai_allow_list_is_absent(self):
        self._customer_says("<p>sensitive</p>")
        self.env.flush_all()
        self.env.cr.execute(
            "UPDATE hams_helpdesk_ticket SET ticket_type = %s WHERE id = %s",
            ("simulated_sensitive_category_not_on_allowlist", self.ticket.id),
        )
        self.ticket.invalidate_recordset()
        self.assertEqual(self._ask({str(self.ticket.id): self.long_ago}), {})

    def test_06_output_is_bounded_to_three_messages_of_2000_characters(self):
        for _ in range(5):
            self._customer_says("<p>%s</p>" % ("x" * 5000))
        shown = self._ask({str(self.ticket.id): self.long_ago})[str(self.ticket.id)]
        self.assertEqual(len(shown), 3)
        self.assertTrue(all(len(m["body"]) <= 2000 for m in shown))

    def test_07_garbage_input_is_ignored_and_the_call_writes_nothing(self):
        before = self.env["mail.message"].search_count([])
        for bad in (None, [], "x", {"abc": self.long_ago}, {str(self.ticket.id): "not a date"}, {str(self.ticket.id): None}):
            self.assertEqual(self._ask(bad), {})
        self.assertEqual(self.env["mail.message"].search_count([]), before)

    def test_08_a_portal_customers_own_reply_counts(self):
        self.ticket.with_user(self.customer_user).message_post(
            body="<p>portal reply</p>", message_type="comment", subtype_xmlid="mail.mt_comment",
        )
        result = self._ask({str(self.ticket.id): self.long_ago})
        self.assertIn("portal reply", result[str(self.ticket.id)][0]["body"])
