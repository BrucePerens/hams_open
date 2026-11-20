# Copyright © Bruce Perens K6BP.
# SPDX-License-Identifier: AGPL-3.0-or-later

# -*- coding: utf-8 -*-
"""Who may see, close and open tickets on the /my/ticket pages: a member reaches only their own tickets, and a
forged POST body cannot make a ticket of a type the portal withholds."""
import re

from odoo.addons.zero_sudo.tests.common import HamsHttpCase
from odoo.tests.common import tagged


@tagged("post_install", "-at_install")
class TestPortalTicketAccess(HamsHttpCase):
    def setUp(self):
        super().setUp()
        portal = self.env.ref("base.group_portal")
        self.member = self.env["res.users"].create(
            {"name": "Access Member", "login": "helpdesk_access_member", "password": "helpdesk_access_member",
             "group_ids": [(6, 0, [portal.id])]}
        )
        self.other = self.env["res.users"].create(
            {"name": "Access Other", "login": "helpdesk_access_other", "password": "helpdesk_access_other",
             "group_ids": [(6, 0, [portal.id])]}
        )
        Ticket = self.env["hams_helpdesk.ticket"]
        self.mine = Ticket.create({"name": "Access Mine", "description": "<p>x</p>", "partner_id": self.member.partner_id.id})
        self.theirs = Ticket.create(
            {"name": "Access Secret Subject", "description": "<p>x</p>", "partner_id": self.other.partner_id.id}
        )
        self.authenticate("helpdesk_access_member", "helpdesk_access_member")

    def _csrf(self):
        page = self.url_open("/my/ticket/%s" % self.mine.id).text
        match = re.search(r'name="csrf_token"\s+value="([^"]+)"', page)
        self.assertTrue(match, "the ticket page carries a CSRF token")
        return match.group(1)

    def _post(self, path, **data):
        data["csrf_token"] = self._csrf()
        return self.url_open(path, data=data, allow_redirects=False)

    # Tests [@ANCHOR: hams_helpdesk:COMM_portal_ticket_detail]
    def test_a_member_cannot_open_another_members_ticket(self):
        response = self.url_open("/my/ticket/%s" % self.theirs.id, allow_redirects=False)
        self.assertEqual(response.status_code, 303)
        self.assertTrue(response.headers["Location"].endswith("/my"), response.headers["Location"])
        self.assertNotIn("Access Secret Subject", response.text)

    # Tests [@ANCHOR: hams_helpdesk:COMM_portal_ticket_close]
    def test_a_member_cannot_close_another_members_ticket(self):
        response = self._post("/my/ticket/%s/close" % self.theirs.id)
        self.assertEqual(response.status_code, 303)
        self.assertTrue(response.headers["Location"].endswith("/my"), response.headers["Location"])
        self.theirs.invalidate_recordset()
        self.assertNotEqual(self.theirs.stage, "closed")

    # Tests [@ANCHOR: hams_helpdesk:COMM_portal_ticket_close]
    def test_a_member_can_close_their_own_ticket(self):
        response = self._post("/my/ticket/%s/close" % self.mine.id)
        self.assertEqual(response.status_code, 303)
        self.assertTrue(response.headers["Location"].endswith("/my/ticket/%s" % self.mine.id))
        self.mine.invalidate_recordset()
        self.assertEqual(self.mine.stage, "closed")

    # Tests [@ANCHOR: hams_helpdesk:COMM_portal_ticket_new]
    def test_a_submission_without_a_subject_creates_nothing(self):
        before = self.env["hams_helpdesk.ticket"].search_count([])
        response = self._post("/my/tickets/submit", name="", description="no subject")
        self.assertEqual(response.status_code, 303)
        self.assertTrue(response.headers["Location"].endswith("/my/tickets/new"))
        self.assertEqual(self.env["hams_helpdesk.ticket"].search_count([]), before)

    # Tests [@ANCHOR: hams_helpdesk:COMM_portal_ticket_new]
    def test_a_forged_ticket_type_falls_back_to_general(self):
        forged_types = ("server_exception", "csam_enticement_trafficking", "no_such_type")
        for forged in forged_types:
            self._post("/my/tickets/submit", name="Forged type %s" % forged, description="d", ticket_type=forged)
        tickets = self.env["hams_helpdesk.ticket"].search([("name", "in", ["Forged type %s" % f for f in forged_types])])
        self.assertEqual(len(tickets), 3)
        self.assertEqual(set(tickets.mapped("ticket_type")), {"general"})

    # Tests [@ANCHOR: hams_helpdesk:COMM_portal_ticket_detail]
    # Tests [@ANCHOR: hams_helpdesk:COMM_portal_ticket_close]
    # Tests [@ANCHOR: hams_helpdesk:portal_my_tickets]
    def test_the_reported_member_cannot_see_or_close_a_mandatory_report_ticket(self):
        # The partner of a mandatory-report ticket is the REPORTED member: seeing, closing or being mailed about it tips them off.
        report = self.env["hams_helpdesk.ticket"].create(
            {"name": "Mandatory Report Subject", "description": "<p>x</p>", "partner_id": self.member.partner_id.id,
             "ticket_type": "csam_enticement_trafficking"}
        )
        listing = self.url_open("/my/tickets").text
        self.assertIn("Access Mine", listing)
        self.assertNotIn("Mandatory Report Subject", listing)
        detail = self.url_open("/my/ticket/%s" % report.id, allow_redirects=False)
        self.assertEqual(detail.status_code, 303)
        self.assertNotIn("Mandatory Report Subject", detail.text)
        self._post("/my/ticket/%s/close" % report.id)
        report.invalidate_recordset()
        self.assertNotEqual(report.stage, "closed")
        self.assertFalse(report.with_user(self.member).search_count([("id", "=", report.id)]), "the portal rule hides it")

    # Tests [@ANCHOR: hams_helpdesk:portal_my_tickets]
    def test_tickets_created_together_page_without_repeats_or_gaps(self):
        # Tickets made in one transaction share a create_date: the page order must still be total (newest id first).
        Ticket = self.env["hams_helpdesk.ticket"]
        made = Ticket.create(
            [{"name": "Tie Order %02d" % n, "description": "<p>x</p>", "partner_id": self.member.partner_id.id}
             for n in range(45)]
        )
        self.env.cr.execute("UPDATE hams_helpdesk_ticket SET create_date = %s WHERE id IN %s", ("2026-01-01 00:00:00", tuple(made.ids)))
        seen = []
        for path in ("/my/tickets", "/my/tickets/page/2", "/my/tickets/page/3"):
            seen += re.findall(r"Tie Order (\d\d)", self.url_open(path).text)
        expected = ["%02d" % n for n in sorted(range(45), reverse=True)]
        self.assertEqual(seen, expected, "every ticket exactly once, in id order within the shared create_date")
