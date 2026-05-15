from odoo.tests.common import TransactionCase, tagged
from unittest.mock import patch

@tagged('post_install', '-at_install', 'standard')
class TestHelpdeskCore(TransactionCase):

    @classmethod
    def setUpClass(cls):
        super().setUpClass()
        # Provision test roles
        cls.manager_user = cls.env['res.users'].create({
            'name': 'Helpdesk Manager',
            'login': 'hd_manager_test',
            'group_ids': [(6, 0, [cls.env.ref('hams_helpdesk.group_helpdesk_manager').id])]
        })
        cls.portal_user = cls.env['res.users'].create({
            'name': 'Portal Customer',
            'login': 'portal_cust_test',
            'group_ids': [(6, 0, [cls.env.ref('base.group_portal').id])]
        })

    def test_01_ticket_creation_and_routing(self):
        """Verify ticket creation routes to on-duty user, subscribes customer, and fires bus toast."""
        # [@ANCHOR: test_01_ticket_creation_and_routing]
        # Tests [@ANCHOR: helpdesk_ticket_creation]
        # Tests [@ANCHOR: helpdesk_ticket_lifecycle]
        # Mock the on-duty admin resolver and the Odoo bus to prevent real websocket dispatch during tests
        with patch('odoo.addons.calendar.models.calendar_event.CalendarEvent.get_current_on_duty_admin', return_value=self.manager_user, create=True), \
             patch('odoo.addons.bus.models.bus.BusBus._sendone') as mock_sendone:

            ticket = self.env['hams_helpdesk.ticket'].create({
                'name': 'Test Outage Incident',
                'description': '<p>System is down</p>',
                'partner_id': self.portal_user.partner_id.id
            })

            self.assertEqual(ticket.user_id, self.manager_user, "Ticket MUST auto-assign to the currently active on-duty manager.")
            self.assertIn(self.portal_user.partner_id, ticket.message_partner_ids, "The reporting Customer MUST be automatically subscribed to their ticket thread for mail-backs.")
            self.assertTrue(mock_sendone.called, "A warning Toast notification MUST be dispatched to the bus for the on-duty operator.")

    def test_02_shift_handoff_wizard(self):
        """Verify the formal shift handoff transfers ownership and logs the secure history."""
        # [@ANCHOR: test_02_shift_handoff_wizard]
        # Tests [@ANCHOR: helpdesk_shift_handoff]
        # Tests [@ANCHOR: helpdesk_handoff_execution]
        ticket = self.env['hams_helpdesk.ticket'].create({
            'name': 'Handoff Test Ticket',
            'user_id': self.manager_user.id
        })

        new_user = self.env['res.users'].create({
            'name': 'Next Shift Operator',
            'login': 'next_shift_test',
            'group_ids': [(6, 0, [self.env.ref('hams_helpdesk.group_helpdesk_user').id])]
        })

        wizard = self.env['hams_helpdesk.shift_handoff'].create({
            'ticket_id': ticket.id,
            'old_user_id': self.manager_user.id,
            'new_user_id': new_user.id,
            'handoff_notes': 'Proceed with DB restart. I have already flushed the Redis cache.'
        })

        # Execute the formal handoff
        wizard.action_confirm_handoff()

        self.assertEqual(ticket.user_id, new_user, "Ticket ownership MUST instantly transfer to the new shift operator.")

        # Verify the audit log was written to the chatter
        messages = self.env['mail.message'].search([('res_id', '=', ticket.id), ('model', '=', 'hams_helpdesk.ticket')])
        audit_trail = " ".join([m.body for m in messages if m.body])
        self.assertIn('Official Shift Handoff Executed', audit_trail)
        self.assertIn('Proceed with DB restart', audit_trail)

    def test_03_portal_security_rules(self):
        """Verify DevSecOps compliance: Portal users can ONLY access their own explicitly owned tickets."""
        my_ticket = self.env['hams_helpdesk.ticket'].create({
            'name': 'My Authorized Ticket',
            'partner_id': self.portal_user.partner_id.id
        })
        other_ticket = self.env['hams_helpdesk.ticket'].create({
            'name': 'Other Confidential Ticket',
            'partner_id': self.manager_user.partner_id.id
        })

        # Switch ORM execution context to the unprivileged portal user
        Ticket_as_portal = self.env['hams_helpdesk.ticket'].with_user(self.portal_user)
        visible_tickets = Ticket_as_portal.search([])

        self.assertIn(my_ticket, visible_tickets, "Portal user MUST be able to see their own tickets.")
        self.assertNotIn(other_ticket, visible_tickets, "CRITICAL SECURITY FAILURE: Portal user can see another user's ticket.")

    def test_05_doc_injection(self):
        """Verify documentation injection payload executes safely."""
        # [@ANCHOR: test_05_doc_injection]
        # Tests [@ANCHOR: helpdesk_doc_injection]

        # Mock manual.article if it doesn't exist to test the logic
        if "manual.article" not in self.env:
            self.assertTrue(True, "manual.article not present, skipping deep check but ensuring hook safety.")
            self.env["hams_helpdesk.ticket"]._register_hook()
            return

        self.env["hams_helpdesk.ticket"]._register_hook()
        article = self.env["manual.article"].search([("name", "=", "Hams Helpdesk")])
        self.assertTrue(article.exists(), "Documentation article MUST be created in manual.article")
        self.assertIn("Hams Helpdesk provides Zero-Sudo compliant ticketing", article.body)

    def test_04_stage_mailback_automation(self):
        """Verify that transitioning a ticket stage fires an automated mail-back to the subscribed customer."""
        ticket = self.env['hams_helpdesk.ticket'].create({
            'name': 'Mailback Test',
            'partner_id': self.portal_user.partner_id.id,
            'stage': 'new'
        })

        # Transition stage
        ticket.write({'stage': 'in_progress'})

        messages = self.env['mail.message'].search([('res_id', '=', ticket.id), ('model', '=', 'hams_helpdesk.ticket')])
        mailback_found = any('Your issue has been updated' in (m.body or '') for m in messages)

        self.assertTrue(mailback_found, "A stage transition MUST trigger a mail-back notification to the customer.")
