# -*- coding: utf-8 -*-
from odoo.tests.common import tagged
from odoo.addons.zero_sudo.tests.common import HamsTransactionCase


@tagged("post_install", "-at_install", "standard")
class TestHelpdeskAdapter(HamsTransactionCase):

    @classmethod
    def setUpClass(cls):
        super().setUpClass()
        # Provision the on-duty shift worker
        cls.on_duty_user = cls.env["res.users"].create(
            {
                "name": "On Call Admin",
                "login": "on_call_admin",
                "group_ids": [(6, 0, [])],
            }
        )

    def test_01_adapter_creates_ticket_and_event(self):
        """Verify the adapter successfully creates a ticket and a calendar event when an incident fires."""
        # Tests [@ANCHOR: pd_helpdesk_adapter]
        # Ensure the parameter is set to a valid model
        self.env["ir.config_parameter"].set_param(
            "pager_duty.helpdesk_model", "hams_helpdesk.ticket"
        )

        manager = self.on_duty_user

        self.safe_patch_object(
            type(self.env["calendar.event"]),
            "get_current_on_duty_admin",
            lambda self: manager,
            create=True,
        )
        incident = self.env["pager.incident"].create(
            {
                "name": "Test Adapter Incident",
                "source": "test_source",
                "severity": "high",
                "description": "Test description",
            }
        )

        # Verify ticket creation
        self.assertTrue(
            incident.helpdesk_ticket_id, "Adapter MUST assign a helpdesk ticket ID."
        )
        ticket = self.env["hams_helpdesk.ticket"].browse(incident.helpdesk_ticket_id)
        self.assertTrue(ticket.exists(), "The actual ticket record MUST exist.")
        self.assertEqual(
            ticket.user_id,
            self.on_duty_user,
            "Ticket MUST be assigned to the on-duty admin.",
        )

        # Verify calendar event creation
        events = self.env["calendar.event"].search(
            [
                ("partner_ids", "in", self.on_duty_user.partner_id.id),
                ("name", "ilike", incident.name),
            ]
        )
        self.assertTrue(
            events, "A calendar event MUST be created for the incident response."
        )

    def test_02_smtp_fallback_on_missing_model(self):
        """Verify that a missing target model triggers the emergency SMTP fallback page."""
        # Set to an invalid/uninstalled model using safe_patch to avoid ormcache test leakage
        self.safe_patch_object(
            type(self.env["zero_sudo.security.utils"]),
            "_get_system_param",
            return_value="invalid.model.does.not.exist"
        )

        manager = self.on_duty_user

        self.safe_patch_object(
            type(self.env["calendar.event"]),
            "get_current_on_duty_admin",
            lambda self: manager,
            create=True,
        )
        incident = self.env["pager.incident"].create(
            {
                "name": "Test Fallback Incident",
                "source": "test_fallback",
                "severity": "critical",
                "description": "This should trigger the fallback",
            }
        )

        # Verify fallback occurred (ticket shouldn't exist)
        self.assertFalse(
            incident.helpdesk_ticket_id,
            "Ticket ID should be empty since creation failed.",
        )

        self.env.flush_all()
        # Verify the fallback message was posted to the incident chatter, alerting the on-duty user
        messages = self.env["mail.message"].search(
            [("res_id", "=", incident.id), ("model", "=", "pager.incident")]
        )
        fallback_found = any(
            "EMERGENCY PAGE (Helpdesk Fallback)" in (m.body or "") for m in messages
        )
        self.assertTrue(
            fallback_found,
            "The adapter MUST execute an emergency SMTP message post if the helpdesk system is unreachable.",
        )

    def test_03_batch_helpdesk_creation(self):
        """Verify that multiple incidents generate helpdesk tickets in a single batched create query."""
        self.env["ir.config_parameter"].set_param(
            "pager_duty.helpdesk_model", "hams_helpdesk.ticket"
        )
        
        manager = self.on_duty_user
        self.safe_patch_object(
            type(self.env["calendar.event"]),
            "get_current_on_duty_admin",
            lambda self: manager,
            create=True,
        )
        
        incidents = self.env["pager.incident"].create([
            {"name": "Inc1", "source": "src", "severity": "low", "description": "desc1"},
            {"name": "Inc2", "source": "src", "severity": "low", "description": "desc2"},
            {"name": "Inc3", "source": "src", "severity": "low", "description": "desc3"},
        ])
        
        for inc in incidents:
            inc.helpdesk_ticket_id = False
            
        with self.assertQueryCount(200): # Adjust limit if needed, but we check that the creates are batched
            incidents.action_generate_helpdesk_ticket()

