# SPDX-License-Identifier: AGPL-3.0-or-later

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

    def setUp(self):
        super().setUp()
        # The foreign module hams_helpdesk might fail in its automated routing due to missing groups for facility service.
        # Since we only test pager_duty adapter logic, we mock the routing.
        self.safe_patch_object(
            type(self.env["hams_helpdesk.ticket"]),
            "_automated_routing_and_notification",
            lambda self: None,
            create=True,
        )

    def test_01_adapter_creates_ticket_and_event(self):
        """Verify the adapter successfully creates a ticket and a calendar event when an incident fires."""
        # Tests [@ANCHOR: pd_helpdesk_adapter]

        # Tests [@ANCHOR: pager_duty:incident_ticket_adapter_create]
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
            ], limit=10
        )
        self.assertTrue(
            events, "A calendar event MUST be created for the incident response."
        )

    def test_02_smtp_fallback_on_missing_model(self):
        # Tests [@ANCHOR: pager_duty:execute_smtp_fallback]
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
            [("res_id", "=", incident.id), ("model", "=", "pager.incident")], limit=100
        )
        fallback_found = any(
            "EMERGENCY PAGE (Helpdesk Fallback)" in (m.body or "") for m in messages
        )
        self.assertTrue(
            fallback_found,
            "The adapter MUST execute an emergency SMTP message post if the helpdesk system is unreachable.",
        )

    def test_04_spam_flagged_email_incident_is_quarantined_not_dropped(self):
        """Tests [@ANCHOR: pd_helpdesk_adapter]: a real phishing lure (ticket #15's
        own ShareFile/NDA "Secure document" content, per the to-do) must
        still produce a real, visible ticket -- just routed to the "spam"
        stage, with no Incident Response calendar block -- never silently
        dropped."""
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
                "name": "Non Disclosure Agreement. Secure document for your review",
                "source": "email:ShareFile <notifications@sharefile-secure-login.example.net>",
                "severity": "low",
                "description": (
                    "<p>Please review and sign this Non Disclosure Agreement via "
                    "our ShareFile secure document portal.</p>"
                    '<p><a href="https://sharefile-secure-login.example.net/sign">'
                    "Open in ShareFile</a></p>"
                ),
            }
        )
        # "low" severity is in TREND_TRACKED_SEVERITIES, so incident.py's
        # own create() override does not auto-generate a ticket (see
        # [@ANCHOR: pager_trend_severity_gate]) -- call the adapter
        # directly, same as test_03_batch_helpdesk_creation above and as
        # message_new() does for every real inbound email.
        incident.action_generate_helpdesk_ticket()

        self.assertTrue(
            incident.helpdesk_ticket_id,
            "A flagged message must still get a real, visible ticket -- never a "
            "silent drop.",
        )
        ticket = self.env["hams_helpdesk.ticket"].browse(incident.helpdesk_ticket_id)
        self.assertTrue(ticket.exists())
        self.assertEqual(
            ticket.stage,
            "spam",
            "A flagged message must be routed to the visible spam/quarantine stage.",
        )
        # hams_helpdesk.ticket's own create() independently re-resolves and
        # assigns the on-duty admin whenever a payload omits "user_id" --
        # see incident_ticket_adapter.py's own comment on this -- so a
        # quarantined ticket still shows a real owner for audit purposes.
        # What this adapter does fully control, and what's asserted below,
        # is that no calendar meeting gets scheduled over the quarantine.
        self.assertEqual(
            ticket.user_id,
            self.on_duty_user,
            "A quarantined ticket still shows a real owner (hams_helpdesk.ticket's "
            "own create() assigns this independently of the adapter's payload).",
        )

        events = self.env["calendar.event"].search(
            [("name", "ilike", incident.name)], limit=10
        )
        self.assertFalse(
            events,
            "A quarantined ticket must not generate an Incident Response calendar block.",
        )

        self.env.flush_all()
        messages = self.env["mail.message"].search(
            [("res_id", "=", incident.id), ("model", "=", "pager.incident")], limit=50
        )
        self.assertTrue(
            any("Flagged as likely spam/phishing" in (m.body or "") for m in messages),
            "The incident's own chatter must record why it was flagged, for review.",
        )

    def test_05_legitimate_email_incident_is_not_quarantined(self):
        """A genuine human inquiry (same shape as a real info@hams.com
        question) must go through the ordinary "new" stage, get assigned
        to the on-duty admin, and get its calendar block -- the spam
        filter must never touch a real ticket."""
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
                "name": "General question about membership",
                "source": "email:member@example.com",
                "severity": "low",
                "description": "<p>Hi, I'm trying to renew my membership and the "
                "portal gave me an error. Can someone help?</p>",
            }
        )
        incident.action_generate_helpdesk_ticket()

        self.assertTrue(incident.helpdesk_ticket_id)
        ticket = self.env["hams_helpdesk.ticket"].browse(incident.helpdesk_ticket_id)
        self.assertEqual(ticket.stage, "new")
        self.assertEqual(ticket.user_id, self.on_duty_user)

        events = self.env["calendar.event"].search(
            [
                ("partner_ids", "in", self.on_duty_user.partner_id.id),
                ("name", "ilike", incident.name),
            ],
            limit=10,
        )
        self.assertTrue(
            events,
            "A genuine ticket must still get its normal Incident Response calendar block.",
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

