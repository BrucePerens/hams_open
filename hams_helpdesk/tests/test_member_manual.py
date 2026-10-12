# Copyright © Bruce Perens K6BP.
# SPDX-License-Identifier: AGPL-3.0-or-later

# -*- coding: utf-8 -*-
"""The member manual for the Support menu item.

The manual names buttons, boxes and behaviours of the /my/tickets pages. This test
renders those pages as a member and checks that every name the manual uses is on them,
so the manual cannot drift away from the screens.
"""
import os
import re

from odoo.tests.common import tagged
from odoo.addons.zero_sudo.tests.common import HamsHttpCase

MANUAL_PATH = os.path.join(
    os.path.dirname(__file__), "..", "data", "member_manual.html"
)
MANUAL_TITLE = "Getting help: Support tickets"
DEVELOPER_TITLE = "Hams Helpdesk"


@tagged("post_install", "-at_install")
class TestMemberManual(HamsHttpCase):
    def setUp(self):
        super().setUp()
        self.member = self.env["res.users"].create(
            {
                "name": "Manual Member",
                "login": "helpdesk_manual_member",
                "password": "helpdesk_manual_member",
                "group_ids": [(6, 0, [self.env.ref("base.group_portal").id])],
            }
        )
        self.other = self.env["res.users"].create(
            {
                "name": "Manual Other Member",
                "login": "helpdesk_manual_other",
                "password": "helpdesk_manual_other",
                "group_ids": [(6, 0, [self.env.ref("base.group_portal").id])],
            }
        )
        Ticket = self.env["hams_helpdesk.ticket"]
        self.ticket = Ticket.create(
            {
                "name": "Manual Probe Ticket",
                "description": "<p>Probe.</p>",
                "partner_id": self.member.partner_id.id,
            }
        )
        self.others_ticket = Ticket.create(
            {
                "name": "Manual Other Ticket",
                "description": "<p>Probe.</p>",
                "partner_id": self.other.partner_id.id,
            }
        )

    # [@ANCHOR: test_member_manual_is_a_published_member_article]
    # Tests [@ANCHOR: hams_helpdesk:UX_SUPPORT_MAIN]
    def test_01_manual_is_a_published_member_article(self):
        Article = self.env["knowledge.article"]
        manual = Article.search([("name", "=", MANUAL_TITLE)])
        self.assertEqual(len(manual), 1)
        self.assertEqual(manual.audience, "member")
        self.assertTrue(manual.is_published)
        self.assertTrue(manual.help_topic)
        developer = Article.search([("name", "=", DEVELOPER_TITLE)])
        self.assertTrue(developer)
        for article in developer:
            self.assertEqual(article.audience, "internal")
            self.assertFalse(article.is_published)

    # Tests [@ANCHOR: hams_helpdesk:UX_SUPPORT_WHERE]
    def test_02_list_page_has_the_names_the_manual_uses(self):
        self.authenticate("helpdesk_manual_member", "helpdesk_manual_member")
        body = self.url_open("/my/tickets").text
        for name in (
            "New Ticket",
            "Ticket #",
            "Title",
            "Callsign",
            "Priority",
            "Stage",
            "Manual Probe Ticket",
        ):
            self.assertIn(name, body, name)
        self.assertNotIn("Manual Other Ticket", body)

    # Tests [@ANCHOR: hams_helpdesk:UX_SUPPORT_OPEN]
    # Tests [@ANCHOR: hams_helpdesk:UX_SUPPORT_CATEGORIES]
    def test_03_new_ticket_form_has_the_names_the_manual_uses(self):
        self.authenticate("helpdesk_manual_member", "helpdesk_manual_member")
        body = self.url_open("/my/tickets/new").text
        for name in (
            "Subject",
            "Callsign",
            "Category",
            "Description",
            "Submit Ticket",
            "General Inquiry",
            "hams_local_relay: Bug / Security Report",
        ):
            self.assertIn(name, body, name)
        self.assertNotIn('type="file"', body)
        self.assertNotIn("NCMEC", body)

    # Tests [@ANCHOR: hams_helpdesk:UX_SUPPORT_FOLLOW]
    # Tests [@ANCHOR: hams_helpdesk:UX_SUPPORT_CLOSE]
    def test_04_ticket_page_has_the_names_the_manual_uses(self):
        self.authenticate("helpdesk_manual_member", "helpdesk_manual_member")
        body = self.url_open("/my/ticket/%s" % self.ticket.id).text
        for name in (
            "Status",
            "Close Ticket",
            "Communication History",
            "o_portal_chatter",
        ):
            self.assertIn(name, body, name)
        self.ticket.write({"stage": "closed"})
        closed = self.url_open("/my/ticket/%s" % self.ticket.id).text
        self.assertNotIn("Close Ticket", closed)

    # Tests [@ANCHOR: hams_helpdesk:UX_SUPPORT_PRIVACY]
    def test_05_a_member_cannot_open_anothers_ticket(self):
        self.authenticate("helpdesk_manual_member", "helpdesk_manual_member")
        response = self.url_open(
            "/my/ticket/%s" % self.others_ticket.id, allow_redirects=False
        )
        self.assertIn(response.status_code, (301, 302, 303))
        self.assertTrue(response.headers["Location"].endswith("/my"))

    # Tests [@ANCHOR: hams_helpdesk:UX_SUPPORT_FOLLOW]
    def test_06_a_member_can_post_a_message_on_their_own_ticket(self):
        self.authenticate("helpdesk_manual_member", "helpdesk_manual_member")
        result = self.make_jsonrpc_request(
            "/mail/message/post",
            {
                "thread_model": "hams_helpdesk.ticket",
                "thread_id": self.ticket.id,
                "post_data": {"body": "Manual reply probe", "message_type": "comment"},
            },
        )
        self.assertTrue(result.get("message_id"))
        message = self.env["mail.message"].browse(result["message_id"])
        self.assertEqual(message.res_id, self.ticket.id)
        self.assertEqual(message.author_id, self.member.partner_id)

    # Tests [@ANCHOR: hams_helpdesk:UX_SUPPORT_STATUS]
    # Tests [@ANCHOR: hams_helpdesk:UX_SUPPORT_EMAIL]
    def test_07_status_names_and_update_email(self):
        stages = dict(self.env["hams_helpdesk.ticket"]._fields["stage"].selection)
        for label in (
            "New",
            "In Progress",
            "Resolved",
            "Closed",
            "Spam / Phishing (Quarantined)",
        ):
            self.assertIn(label, stages.values())
        self.ticket.write({"stage": "in_progress"})
        update = self.ticket.message_ids.filtered(
            lambda m: m.subject == "Ticket Update: Manual Probe Ticket"
        )
        self.assertTrue(update)
        self.assertIn("New Status: In Progress", update[0].body)
        self.assertIn(self.member.partner_id, update[0].partner_ids)

    # Tests [@ANCHOR: hams_helpdesk:UX_SUPPORT_REPORTS]
    # Tests [@ANCHOR: hams_helpdesk:UX_SUPPORT_NEXT]
    def test_08_manual_anchors_are_unique_and_the_file_is_clean(self):
        with open(MANUAL_PATH, encoding="utf-8") as handle:
            html = handle.read()
        anchors = re.findall(r"\[@ANCHOR: (hams_helpdesk:UX_SUPPORT_\w+)\]", html)
        self.assertTrue(anchors)
        self.assertEqual(len(anchors), len(set(anchors)))
        self.assertNotIn("style=", html)
        self.assertNotIn("<img", html)
