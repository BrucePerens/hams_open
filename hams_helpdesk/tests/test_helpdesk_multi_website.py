# -*- coding: utf-8 -*-
from odoo.tests.common import tagged
from odoo.addons.zero_sudo.tests.common import HamsTransactionCase


@tagged("post_install", "-at_install", "standard")
class TestHelpdeskMultiWebsite(HamsTransactionCase):

    def setUp(self):
        super().setUp()
        self.website_1 = self.env["website"].create({"name": "Website 1"})
        self.website_2 = self.env["website"].create({"name": "Website 2"})

        self.portal_partner = self.env["res.partner"].create(
            {
                "name": "Portal Customer",
                "email": "portal@example.com",
            }
        )
        self.portal_user = self.env["res.users"].create(
            {
                "name": "Portal Customer",
                "login": "portal_multi_website",
                "partner_id": self.portal_partner.id,
                "group_ids": [(6, 0, [self.env.ref("base.group_portal").id])],
            }
        )

    def test_06_multi_website_awareness_logic(self):
        """Verify tickets are correctly segregated by website_id in multi-website environments."""
        # Tests [@ANCHOR: helpdesk_multi_website]
        # Tests [@ANCHOR: multi_website_segregation]
        # [@ANCHOR: test_06_multi_website_awareness_logic]

        # Ensure company context is consistent
        self.portal_user.company_id = self.env.company

        ticket_w1 = self.env["hams_helpdesk.ticket"].create(
            {
                "name": "Website 1 Ticket",
                "partner_id": self.portal_partner.id,
                "website_id": self.website_1.id,
                "company_id": self.env.company.id,
            }
        )
        ticket_w2 = self.env["hams_helpdesk.ticket"].create(
            {
                "name": "Website 2 Ticket",
                "partner_id": self.portal_partner.id,
                "website_id": self.website_2.id,
                "company_id": self.env.company.id,
            }
        )
        ticket_no_w = self.env["hams_helpdesk.ticket"].create(
            {
                "name": "No Website Ticket",
                "partner_id": self.portal_partner.id,
                "website_id": False,
                "company_id": self.env.company.id,
            }
        )

        # Ensure all data is flushed and cache is cleared
        self.env.flush_all()
        self.env.invalidate_all()

        # Test portal user on Website 1
        self.portal_user.write({"website_id": self.website_1.id})
        self.env.flush_all()
        self.env.invalidate_all()

        Ticket_portal_w1 = (
            self.env["hams_helpdesk.ticket"]
            .with_user(self.portal_user.id)
            .with_company(self.env.company.id)
            .with_context(website_id=self.website_1.id)
        )

        visible_w1 = Ticket_portal_w1.search([])
        self.assertIn(
            ticket_w1,
            visible_w1,
            "Portal user on Website 1 should see Website 1 ticket",
        )
        self.assertIn(
            ticket_no_w, visible_w1, "Portal user should see tickets with no website"
        )
        self.assertNotIn(
            ticket_w2,
            visible_w1,
            "Portal user on Website 1 should NOT see tickets from Website 2",
        )

        # Test portal user on Website 2
        self.portal_user.write({"website_id": self.website_2.id})
        self.env.flush_all()
        self.env.invalidate_all()

        Ticket_portal_w2 = (
            self.env["hams_helpdesk.ticket"]
            .with_user(self.portal_user.id)
            .with_company(self.env.company.id)
            .with_context(website_id=self.website_2.id)
        )
        visible_w2 = Ticket_portal_w2.search([])
        self.assertIn(
            ticket_w2,
            visible_w2,
            "Portal user on Website 2 should see Website 2 ticket",
        )
        self.assertIn(
            ticket_no_w, visible_w2, "Portal user should see tickets with no website"
        )
        self.assertNotIn(
            ticket_w1,
            visible_w2,
            "Portal user on Website 2 should NOT see tickets from Website 1",
        )

    def test_07_ticket_creation_from_context(self):
        """Verify ticket creation correctly picks up website_id from context."""
        ticket = (
            self.env["hams_helpdesk.ticket"]
            .with_context(website_id=self.website_1.id)
            .create(
                {
                    "name": "Context Website Ticket",
                }
            )
        )
        self.assertEqual(
            ticket.website_id,
            self.website_1,
            "Ticket should inherit website_id from context during creation",
        )
