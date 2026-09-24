# This software is distributed under the terms of the Affero General Public License (AGPL-3).

# -*- coding: utf-8 -*-
from odoo.exceptions import AccessError
from odoo.tests.common import tagged
from odoo.addons.zero_sudo.tests.common import HamsTransactionCase


@tagged("post_install", "-at_install", "standard")
class TestHelpdeskAiTriageExternalAllowlist(HamsTransactionCase):
    """Coverage for the external-AI ticket-triage allow-list added
    2026-09-23 -- group_ai_triage_external_service,
    rule_helpdesk_ticket_ai_triage_external_allowlist, and
    hams_helpdesk.ticket.mcp_post_internal_note() -- per
    hams_com's docs/proposals/CHILD_SAFETY_COMMUNICATIONS_CONSENT.md
    section G and night_shift_todo/medium/
    duplicate-service-account-privilege-helpdesk-security-af9e1c30.md
    (both in hams_com). See security/helpdesk_security.xml's own
    group_ai_triage_external_service comment for the full design
    rationale this test suite is verifying."""

    def setUp(self):
        super().setUp()
        self.admin = self.env.ref("base.user_admin")
        self.ai_triage_service = self.env.ref("hams_helpdesk.user_ai_triage_service")
        self.ai_triage_uid = self.env["zero_sudo.security.utils"]._get_service_uid(
            "hams_helpdesk.user_ai_triage_service"
        )
        self.general_ticket = (
            self.env["hams_helpdesk.ticket"]
            .with_user(self.admin)
            .create(
                {
                    "name": "AI triage allowlist test: general",
                    "description": "<p>test</p>",
                    "ticket_type": "general",
                }
            )
        )
        self.relay_ticket = (
            self.env["hams_helpdesk.ticket"]
            .with_user(self.admin)
            .create(
                {
                    "name": "AI triage allowlist test: hams_local_relay",
                    "description": "<p>test</p>",
                    "ticket_type": "hams_local_relay",
                }
            )
        )

    def _make_out_of_allowlist_ticket(self):
        """hams_helpdesk.ticket.ticket_type only ever has two live Selection
        values today ('general'/'hams_local_relay'), both on the
        allow-list, so there is no real third category to construct a
        genuine negative case against yet. Mutating the column directly
        via parameterized SQL (bypassing the ORM's own Selection
        validation, which would reject any other value on write/create)
        is the only way to exercise
        rule_helpdesk_ticket_ai_triage_external_allowlist's actual
        exclusion behavior against a real database row before a real
        third category exists -- this value is never a valid ticket_type
        in production, only a stand-in for "some future sensitive
        category" for this test's purposes."""
        ticket = (
            self.env["hams_helpdesk.ticket"]
            .with_user(self.admin)
            .create(
                {
                    "name": "AI triage allowlist test: excluded category",
                    "description": "<p>test</p>",
                    "ticket_type": "general",
                }
            )
        )
        self.env.flush_all()
        self.env.cr.execute(
            "UPDATE hams_helpdesk_ticket SET ticket_type = %s WHERE id = %s",
            ("simulated_sensitive_category_not_on_allowlist", ticket.id),
        )
        ticket.invalidate_recordset()
        return ticket

    def test_01_service_account_group_membership_replaced_not_shared(self):
        """Tests the duplicate-service-account-privilege fix itself: the
        two service accounts must no longer share group_helpdesk_manager,
        and the AI triage account must not have picked up
        group_helpdesk_user/group_helpdesk_manager/base.group_portal by
        any other path (implied_ids or otherwise) -- any of those would
        OR their own ir.rule in with
        rule_helpdesk_ticket_ai_triage_external_allowlist and silently
        defeat it (see group_ai_triage_external_service's own comment on
        why it's deliberately not implied by/implying those groups)."""
        helpdesk_service = self.env.ref("hams_helpdesk.user_helpdesk_service")
        manager_group = self.env.ref("hams_helpdesk.group_helpdesk_manager")
        external_group = self.env.ref("hams_helpdesk.group_ai_triage_external_service")

        self.assertIn(manager_group, helpdesk_service.group_ids)
        self.assertNotIn(manager_group, self.ai_triage_service.group_ids)
        self.assertIn(external_group, self.ai_triage_service.group_ids)

        # Effective (computed, including implied) groups -- has_group()
        # walks implied_ids, a plain "in group_ids" check does not.
        self.assertFalse(
            self.ai_triage_service.has_group("hams_helpdesk.group_helpdesk_user")
        )
        self.assertFalse(
            self.ai_triage_service.has_group("hams_helpdesk.group_helpdesk_manager")
        )
        self.assertFalse(self.ai_triage_service.has_group("base.group_portal"))

    def test_02_ai_triage_can_read_allowlisted_categories(self):
        """Positive control, using the exact field list
        daemons/hams_ticket_triage_mcp/main.py's _TICKET_FIELDS actually
        requests over RPC (hams_com) -- proves the new group's read grant
        covers the real fields the daemon's list_new_tickets/read_ticket
        tools need, not just an id."""
        fields = ["name", "description", "callsign", "ticket_type", "stage", "priority", "create_date"]
        Ticket = self.env["hams_helpdesk.ticket"].with_user(self.ai_triage_uid)
        rows = Ticket.search_read(
            [("id", "in", [self.general_ticket.id, self.relay_ticket.id])], fields
        )
        self.assertEqual(len(rows), 2)

    def test_03_ai_triage_cannot_see_ticket_outside_allowlist(self):
        """The rule's actual exclusion behavior against a real row -- see
        _make_out_of_allowlist_ticket()'s own docstring for why a raw SQL
        mutation is used here."""
        excluded_ticket = self._make_out_of_allowlist_ticket()
        visible_count = (
            self.env["hams_helpdesk.ticket"]
            .with_user(self.ai_triage_uid)
            .search_count([("id", "=", excluded_ticket.id)])
        )
        self.assertEqual(visible_count, 0)

    def test_04_ai_triage_service_account_can_read_but_never_write_create_or_unlink(self):
        """Same least-privilege standard test_pager_mcp_triage.py's own
        test_04 already holds pager_duty's equivalent account to: allowed
        to read and to call the one method that internally elevates,
        violently rejected by the ORM for anything else."""
        svc_ticket = self.general_ticket.with_user(self.ai_triage_uid)
        svc_ticket.read(["name", "ticket_type"])
        svc_ticket.mcp_post_internal_note("allowed note")

        with self.assertRaises(AccessError):
            svc_ticket.write({"name": "renamed by AI"})
            self.env.flush_all()

        with self.assertRaises(AccessError):
            self.env["hams_helpdesk.ticket"].with_user(self.ai_triage_uid).create(
                {"name": "created by AI", "ticket_type": "general"}
            )
            self.env.flush_all()

        with self.assertRaises(AccessError):
            svc_ticket.unlink()
            self.env.flush_all()

    def test_05_mcp_post_internal_note_posts_as_internal_note_on_allowlisted_ticket(self):
        # Tests [@ANCHOR: hams_helpdesk:mcp_post_internal_note]
        self.general_ticket.with_user(self.ai_triage_uid).mcp_post_internal_note(
            "AI triage analysis: looks routine"
        )
        last_message = self.general_ticket.message_ids.sorted(key=lambda m: m.id)[-1]
        self.assertIn("AI triage analysis: looks routine", last_message.body)
        # mail.mt_note, not mail.mt_comment: an internal note must never be
        # customer-visible (same class of leak fixed in
        # _automated_routing_and_notification's own 2026-09-09 bug-hunt
        # comment on this exact model).
        self.assertTrue(last_message.subtype_id.internal)

    def test_06_mcp_post_internal_note_enforces_allowlist_before_elevating(self):
        """The security-critical case: mcp_post_internal_note() must not
        let the internal elevation to user_helpdesk_service become a way
        to annotate a ticket the caller was never allowed to see in the
        first place. The self.read(["ticket_type"]) inside
        mcp_post_internal_note(), evaluated under the CALLING identity
        before any with_env() elevation, is what's actually being tested
        here."""
        excluded_ticket = self._make_out_of_allowlist_ticket()
        with self.assertRaises(AccessError):
            excluded_ticket.with_user(self.ai_triage_uid).mcp_post_internal_note(
                "should never be posted"
            )
            self.env.flush_all()
