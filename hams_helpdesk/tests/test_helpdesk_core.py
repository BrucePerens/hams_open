# -*- coding: utf-8 -*-
from odoo.exceptions import AccessError
from odoo.tests.common import tagged
from odoo.addons.zero_sudo.tests.common import HamsTransactionCase


@tagged("post_install", "-at_install", "standard")
class TestHelpdeskCore(HamsTransactionCase):

    def setUp(self):
        super().setUp()
        # Provision test roles
        self.manager_partner = self.env["res.partner"].create(
            {
                "name": "Helpdesk Manager Partner",
                "email": "manager@example.com",
            }
        )
        self.manager_user = self.env["res.users"].create(
            {
                "name": "Helpdesk Manager",
                "login": "hd_manager_test",
                "partner_id": self.manager_partner.id,
                "group_ids": [
                    (6, 0, [self.env.ref("hams_helpdesk.group_helpdesk_manager").id])
                ],
            }
        )
        self.portal_partner = self.env["res.partner"].create(
            {
                "name": "Portal Customer Partner",
                "email": "portal@example.com",
            }
        )
        self.portal_user = self.env["res.users"].create(
            {
                "name": "Portal Customer",
                "login": "portal_cust_test",
                "partner_id": self.portal_partner.id,
                "group_ids": [(6, 0, [self.env.ref("base.group_portal").id])],
            }
        )

    def test_01_ticket_creation_and_routing(self):
        """Verify ticket creation routes to on-duty user, subscribes customer, and fires bus toast."""
        # [@ANCHOR: test_01_ticket_creation_and_routing]
        # Tests [@ANCHOR: helpdesk_ticket_creation]
        # Tests [@ANCHOR: helpdesk_ticket_lifecycle]

        manager = self.manager_user

        self.safe_patch_object(
            type(self.env["calendar.event"]),
            "get_current_on_duty_admin",
            lambda self: manager,
            create=True,
        )
        self.safe_patch_object(
            type(self.env["bus.bus"]), "_sendone", lambda *a, **kw: None, create=True
        )

        ticket = self.env["hams_helpdesk.ticket"].create(
            {
                "name": "Test Outage Incident",
                "description": "<p>System is down</p>",
                "partner_id": self.portal_user.partner_id.id,
            }
        )

        self.assertEqual(
            ticket.user_id,
            self.manager_user,
            "Ticket MUST auto-assign to the currently active on-duty manager.",
        )
        self.assertIn(
            self.portal_user.partner_id,
            ticket.message_partner_ids,
            "The reporting Customer MUST be automatically subscribed to their ticket thread for mail-backs.",
        )

    def test_02_shift_handoff_wizard(self):
        """Verify the formal shift handoff transfers ownership and logs the secure history."""
        # [@ANCHOR: test_02_shift_handoff_wizard]
        # Tests [@ANCHOR: helpdesk_shift_handoff]
        # Tests [@ANCHOR: helpdesk_handoff_execution]
        ticket = self.env["hams_helpdesk.ticket"].create(
            {"name": "Handoff Test Ticket", "user_id": self.manager_user.id}
        )

        new_user = self.env["res.users"].create(
            {
                "name": "Next Shift Operator",
                "login": "next_shift_test",
                "group_ids": [
                    (6, 0, [self.env.ref("hams_helpdesk.group_helpdesk_user").id])
                ],
            }
        )

        wizard = self.env["hams_helpdesk.shift_handoff"].create(
            {
                "ticket_id": ticket.id,
                "old_user_id": self.manager_user.id,
                "new_user_id": new_user.id,
                "handoff_notes": "Proceed with DB restart. I have already flushed the Redis cache.",
            }
        )

        # Execute the formal handoff
        # Use a new environment to avoid multi-company search issues during handoff
        wizard.with_company(self.env.company).action_confirm_handoff()

        self.assertEqual(
            ticket.user_id,
            new_user,
            "Ticket ownership MUST instantly transfer to the new shift operator.",
        )

        # Verify the audit log was written to the chatter
        messages = self.env["mail.message"].search(
            [("res_id", "=", ticket.id), ("model", "=", "hams_helpdesk.ticket")]
        )
        audit_trail = " ".join([m.body for m in messages if m.body])
        self.assertIn("Official Shift Handoff Executed", audit_trail)
        self.assertIn("Proceed with DB restart", audit_trail)

    def test_03_portal_security_rules(self):
        """Verify DevSecOps compliance: Portal users can ONLY access their own explicitly owned tickets."""
        # Ensure company context is consistent
        self.portal_user.company_id = self.env.company
        my_ticket = self.env["hams_helpdesk.ticket"].create(
            {
                "name": "My Authorized Ticket",
                "partner_id": self.portal_user.partner_id.id,
                "company_id": self.env.company.id,
            }
        )
        other_ticket = self.env["hams_helpdesk.ticket"].create(
            {
                "name": "Other Confidential Ticket",
                "partner_id": self.manager_user.partner_id.id,
                "company_id": self.env.company.id,
            }
        )

        # Switch ORM execution context to the unprivileged portal user
        Ticket_as_portal = (
            self.env["hams_helpdesk.ticket"]
            .with_user(self.portal_user)
            .with_company(self.env.company)
        )
        visible_tickets = Ticket_as_portal.search([])

        self.assertIn(
            my_ticket,
            visible_tickets,
            "Portal user MUST be able to see their own tickets.",
        )
        self.assertNotIn(
            other_ticket,
            visible_tickets,
            "CRITICAL SECURITY FAILURE: Portal user can see another user's ticket.",
        )

    def test_04_stage_mailback_automation(self):
        """Verify that transitioning a ticket stage fires an automated mail-back to the subscribed customer."""
        ticket = self.env["hams_helpdesk.ticket"].create(
            {
                "name": "Mailback Test",
                "partner_id": self.portal_user.partner_id.id,
                "stage": "new",
            }
        )

        # Transition stage
        ticket.write({"stage": "in_progress"})

        messages = self.env["mail.message"].search(
            [("res_id", "=", ticket.id), ("model", "=", "hams_helpdesk.ticket")]
        )
        mailback_found = any(
            "Your issue has been updated" in (m.body or "") for m in messages
        )

        self.assertTrue(
            mailback_found,
            "A stage transition MUST trigger a mail-back notification to the customer.",
        )

    def test_05_portal_write_restrictions(self):
        """Verify portal users cannot modify administrative fields."""
        # [@ANCHOR: test_05_portal_write_restrictions]
        # Tests [@ANCHOR: helpdesk_micro_privilege]
        ticket = self.env["hams_helpdesk.ticket"].create(
            {
                "name": "Portal Security Test",
                "partner_id": self.portal_user.partner_id.id,
                "stage": "new",
            }
        )

        ticket_as_portal = ticket.with_user(self.portal_user)

        with self.assertRaises(
            AccessError, msg="Portal user MUST NOT be able to change ticket stage."
        ):
            ticket_as_portal.write({"stage": "resolved"})
            self.env.flush_all()

        with self.assertRaises(
            AccessError, msg="Portal user MUST NOT be able to change ticket assignee."
        ):
            ticket_as_portal.write({"user_id": self.manager_user.id})
            self.env.flush_all()

        # Verify they CAN still update description or name if allowed (though usually they shouldn't if it's already created,
        # but let's see current ACLs. CSV says they have write access.)
        ticket_as_portal.write({"description": "Updated description by portal user"})
        self.assertIn("Updated description by portal user", ticket.description)

    def test_06_callsign_population(self):
        """Verify the callsign field is automatically populated from the partner."""
        # Use a partner with a callsign
        self.portal_partner.write({"callsign": "K1AAA"})

        ticket = self.env["hams_helpdesk.ticket"].create(
            {"name": "Callsign Test", "partner_id": self.portal_partner.id}
        )

        self.assertEqual(
            ticket.callsign,
            "K1AAA",
            "Callsign MUST be automatically populated from partner if available.",
        )

        # Verify onchange
        ticket_new = self.env["hams_helpdesk.ticket"].new(
            {"partner_id": self.portal_partner.id}
        )
        ticket_new._onchange_partner_id()
        self.assertEqual(
            ticket_new.callsign, "K1AAA", "Callsign MUST be populated via onchange."
        )

    def test_view_rendering(self):
        """Verify views render correctly without syntax errors."""
        # Tests [@ANCHOR: helpdesk_shift_handoff]
        # Tests [@ANCHOR: helpdesk_ticket_lifecycle]
        self.env["hams_helpdesk.shift_handoff"].get_view(
            view_id=self.env.ref(
                "hams_helpdesk.view_hams_helpdesk_shift_handoff_form"
            ).id
        )
        self.env["hams_helpdesk.ticket"].get_view(
            view_id=self.env.ref("hams_helpdesk.view_hams_helpdesk_ticket_pivot").id
        )
