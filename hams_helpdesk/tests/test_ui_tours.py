from odoo import http
from odoo.tests import tagged
from odoo.addons.zero_sudo.tests.common import HamsHttpCase


@tagged("post_install", "-at_install", "ui", "standard")
class TestHelpdeskTours(HamsHttpCase):

    def setUp(self):
        super().setUp()
        # Provision test roles
        self.manager_user = self.env["res.users"].create(
            {
                "name": "Helpdesk Manager",
                "login": "hd_manager_tour",
                "password": "password",
                "email": "manager_tour@example.com",
                "group_ids": [
                    (6, 0, [self.env.ref("hams_helpdesk.group_helpdesk_manager").id])
                ],
            }
        )
        self.portal_user = self.env["res.users"].create(
            {
                "name": "Portal Customer",
                "login": "portal_cust_tour",
                "password": "password",
                "email": "portal_tour@example.com",
                "group_ids": [(6, 0, [self.env.ref("base.group_portal").id])],
            }
        )

    def test_helpdesk_portal_tour(self):
        """Test portal ticket submission and closure via JS tour."""
        # [@ANCHOR: helpdesk_portal_tour]
        self.start_tour(
            "/my/tickets?debug=1", "helpdesk_portal_tour", login="portal_cust_tour"
        )

    def test_helpdesk_portal_rendering(self):
        """Test portal ticket submission rendering and basic visibility."""
        # Tests [@ANCHOR: helpdesk_portal_new]
        # Tests [@ANCHOR: helpdesk_portal_list]
        # Tests [@ANCHOR: helpdesk_portal_detail]
        self.authenticate("portal_cust_tour", "password")
        # Access portal tickets list
        res = self.url_open("/my/tickets")
        self.assertEqual(
            res.status_code,
            200,
            "[!] DIAGNOSTIC FOR AI: Portal tickets list failed to render (Expected 200).",
        )
        self.assertIn(
            b"Tickets",
            res.content,
            "[!] DIAGNOSTIC FOR AI: 'Tickets' text not found in portal page content.",
        )

        # Access new ticket form
        res = self.url_open("/my/tickets/new")
        self.assertEqual(
            res.status_code,
            200,
            "[!] DIAGNOSTIC FOR AI: New ticket form failed to render (Expected 200).",
        )
        self.assertIn(
            b"Submit New Ticket",
            res.content,
            "[!] DIAGNOSTIC FOR AI: 'Submit New Ticket' header not found in portal.",
        )

    def test_portal_close_ticket(self):
        """Test portal user closing their own ticket."""
        # Tests [@ANCHOR: helpdesk_portal_close]
        ticket = self.env["hams_helpdesk.ticket"].create(
            {
                "name": "Portal Close Test",
                "partner_id": self.portal_user.partner_id.id,
                "stage": "new",
            }
        )
        self.authenticate("portal_cust_tour", "password")
        self.url_open(
            f"/my/ticket/{ticket.id}/close",
            data={"csrf_token": http.Request.csrf_token(self)},
        )
        self.assertEqual(
            ticket.stage, "closed", "Ticket should be closed after portal action."
        )

    def test_helpdesk_operator_tour(self):
        """Test operator backend ticket lifecycle and handoff via JS tour."""
        # [@ANCHOR: helpdesk_operator_tour]
        self.start_tour(
            "/odoo?debug=1&action=hams_helpdesk.action_hams_helpdesk_ticket",
            "helpdesk_operator_tour",
            login="hd_manager_tour",
        )

    def test_helpdesk_operator_rendering(self):
        """Test operator backend facility rendering."""
        # Tests [@ANCHOR: helpdesk_ticket_form]
        # Tests [@ANCHOR: helpdesk_ticket_list]
        ticket = self.env["hams_helpdesk.ticket"].create(
            {
                "name": "Rendering Test Ticket",
                "user_id": self.manager_user.id,
                "description": "Initial description",
            }
        )
        self.authenticate("hd_manager_tour", "password")
        # Direct backend route in Odoo 19
        res = self.url_open(
            f"/odoo?model=hams_helpdesk.ticket&id={ticket.id}&view_type=form"
        )
        self.assertEqual(
            res.status_code,
            200,
            "[!] DIAGNOSTIC FOR AI: Backend ticket form failed to render (Expected 200).",
        )
