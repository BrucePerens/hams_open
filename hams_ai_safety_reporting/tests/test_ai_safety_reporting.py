# This software is distributed under the terms of the Affero General Public License (AGPL-3).

from odoo.exceptions import AccessError
from odoo.tests.common import tagged
from odoo.addons.zero_sudo.tests.common import HamsTransactionCase


@tagged("post_install", "-at_install", "security")
class TestAiSafetyReporting(HamsTransactionCase):
    """Coverage for hams_ai_safety_reporting: the ai_safety_concern ticket_type
    (selection_add on hams_helpdesk.ticket) and group_ai_safety_reporting_service, the narrow
    create-only service account docs/proposals/CHILD_SAFETY_COMMUNICATIONS_CONSENT.md
    (hams_com), section G, Phase 7 item 1 ("bot self-reporting") calls to file a ticket. See
    security/security.xml's own group_ai_safety_reporting_service and
    rule_helpdesk_ticket_ai_safety_reporting_creator comments for the full design rationale this
    suite verifies."""

    def setUp(self):
        super().setUp()
        self.admin = self.env.ref("base.user_admin")
        self.svc_uid = self.env["zero_sudo.security.utils"]._get_service_uid(
            "hams_ai_safety_reporting.user_ai_safety_reporting_service"
        )

    # [@ANCHOR: test_ai_safety_concern_ticket_type_survives_alongside_other_modules]
    def test_ai_safety_concern_ticket_type_survives_alongside_other_modules(self):
        """selection_add, never a bare selection= -- see helpdesk_ticket.py's own comment for
        the bug this guards (a second module's selection= silently replacing the base field's
        whole list, so whichever module loads last wins and the other's value disappears from
        the installed database)."""
        values = dict(self.env["hams_helpdesk.ticket"]._fields["ticket_type"].selection)
        self.assertIn("ai_safety_concern", values)
        self.assertIn("general", values, "hams_helpdesk's own base value must survive")
        self.assertIn(
            "csam_enticement_trafficking",
            values,
            "hams_helpdesk's own core CSAM value must survive",
        )

    # [@ANCHOR: test_ai_safety_reporting_service_account_group_membership_is_narrow]
    def test_service_account_group_membership_is_narrow(self):
        """The account must hold only its own new group (plus base.group_user), never
        group_helpdesk_user/group_helpdesk_manager/base.group_portal -- any of those would OR
        their own ir.rule in with rule_helpdesk_ticket_ai_safety_reporting_creator (Odoo ir.rule
        records for the SAME user are OR'ed, not AND'ed, across every group that user is in) and
        silently defeat the create-only, category-scoped grant this account is supposed to
        have."""
        svc_user = self.env["res.users"].browse(self.svc_uid)
        own_group = self.env.ref("hams_ai_safety_reporting.group_ai_safety_reporting_service")
        self.assertIn(own_group, svc_user.group_ids)
        self.assertFalse(svc_user.has_group("hams_helpdesk.group_helpdesk_user"))
        self.assertFalse(svc_user.has_group("hams_helpdesk.group_helpdesk_manager"))
        self.assertFalse(svc_user.has_group("hams_helpdesk.group_ai_triage_external_service"))
        self.assertFalse(svc_user.has_group("base.group_portal"))

    # [@ANCHOR: test_ai_safety_reporting_service_account_can_create_ai_safety_concern_ticket]
    def test_service_account_can_create_ai_safety_concern_ticket(self):
        """Positive control for this module's actual, immediate real-world use (Phase 7 item 1:
        self_harm / sexual_content / uncivil, via ai_safety_concern)."""
        Ticket = self.env["hams_helpdesk.ticket"].with_user(self.svc_uid)
        ai_ticket = Ticket.create(
            {
                "name": "AI safety flag: uncivil interaction",
                "description": "<p>transcript excerpt / playback link</p>",
                "ticket_type": "ai_safety_concern",
            }
        )
        self.env.flush_all()
        self.assertEqual(ai_ticket.ticket_type, "ai_safety_concern")

    # [@ANCHOR: test_ai_safety_reporting_service_account_can_create_csam_ticket]
    def test_service_account_can_create_a_csam_ticket(self):
        """Real, verified-then-fixed finding (2026-09-24): a create-only account could not
        actually complete create() for a csam_enticement_trafficking ticket, even though
        rule_helpdesk_ticket_ai_safety_reporting_creator's own domain deliberately allows it --
        the block was hams_helpdesk.ticket.create()'s own NCMEC packet-assembly and legal-hold
        steps (COMM_ncmec_packet_and_legal_hold in helpdesk_ticket.py) writing ticket fields
        under the caller's own AMBIENT identity rather than an elevated service account. Every
        caller that had ever exercised this path before (base.user_admin,
        hams_helpdesk.user_helpdesk_service, ad hoc test users in group_helpdesk_manager)
        already held write access on hams_helpdesk.ticket, so the requirement was invisible
        until a genuinely create-only caller -- this module's own account -- tried it.

        Fixed in helpdesk_ticket.py itself (not here): _ncmec_assemble_report_packet() now runs
        under hams_helpdesk.user_helpdesk_service (elevated by create() before calling it), and
        _ncmec_apply_recording_legal_hold_best_effort() elevates its own ticket-field writes
        (ncmec_legal_hold_note/ncmec_legal_hold_applied) to the same identity internally, while
        deliberately leaving the actual cross-repo ham_communications_consent legal-hold call on
        the caller's ambient identity, exactly as that method's own docstring requires (that
        specific AccessError is a separate, real, and correctly-still-open cross-repo permission
        gap -- see its own docstring -- not something a create-only account creating the ticket
        should be blocked by)."""
        Ticket = self.env["hams_helpdesk.ticket"].with_user(self.svc_uid)
        csam_ticket = Ticket.create(
            {
                "name": "AI safety flag: apparent CSAM",
                "description": "<p>transcript excerpt / playback link</p>",
                "ticket_type": "csam_enticement_trafficking",
            }
        )
        self.env.flush_all()
        self.assertEqual(csam_ticket.ticket_type, "csam_enticement_trafficking")
        # The packet-assembly step's own write succeeded (the actual fix under test) --
        # confirmed by the packet existing at all, not just by create() not raising.
        self.assertIn("NCMEC CyberTipline Report Packet", csam_ticket.ncmec_report_packet)

    # [@ANCHOR: test_ai_safety_reporting_service_account_cannot_create_other_categories]
    def test_service_account_cannot_create_a_general_ticket(self):
        """The structural defense-in-depth half: even if a caller-side bug passed
        ticket_type='general' (or omitted it entirely, since 'general' is the field's own
        default), rule_helpdesk_ticket_ai_safety_reporting_creator's domain excludes it, so the
        ORM's own ir.rule create-time check refuses the row regardless of the create-only
        ir.model.access grant."""
        Ticket = self.env["hams_helpdesk.ticket"].with_user(self.svc_uid)
        with self.assertRaises(AccessError):
            Ticket.create({"name": "should be refused", "ticket_type": "general"})
            self.env.flush_all()
        with self.assertRaises(AccessError):
            Ticket.create({"name": "should be refused (default ticket_type)"})
            self.env.flush_all()
        with self.assertRaises(AccessError):
            Ticket.create(
                {"name": "should be refused", "ticket_type": "hams_local_relay"}
            )
            self.env.flush_all()

    # [@ANCHOR: test_ai_safety_reporting_service_account_cannot_read_write_unlink]
    def test_service_account_cannot_read_write_or_unlink_any_ticket(self):
        """Create-only: perm_read=0/perm_write=0/perm_unlink=0 in ir.model.access.csv, so this
        account can't read back, amend, or delete even the ticket it just created itself -- the
        caller (the safety-classification daemon) has no legitimate need to do any of that; only
        hams_helpdesk staff should ever see a filed report again."""
        own_ticket = (
            self.env["hams_helpdesk.ticket"]
            .with_user(self.admin)
            .create(
                {
                    "name": "Pre-existing ticket for read/write/unlink probe",
                    "ticket_type": "ai_safety_concern",
                }
            )
        )
        self.env.flush_all()
        svc_ticket = own_ticket.with_user(self.svc_uid)

        with self.assertRaises(AccessError):
            svc_ticket.read(["name"])

        with self.assertRaises(AccessError):
            svc_ticket.write({"name": "renamed by AI safety reporting service"})
            self.env.flush_all()

        with self.assertRaises(AccessError):
            svc_ticket.unlink()
            self.env.flush_all()

        # Not just a pre-existing ticket -- the SAME account's own just-created row too.
        newly_created = (
            self.env["hams_helpdesk.ticket"]
            .with_user(self.svc_uid)
            .create(
                {
                    "name": "AI safety flag: just created by the service account itself",
                    "ticket_type": "ai_safety_concern",
                }
            )
        )
        self.env.flush_all()
        with self.assertRaises(AccessError):
            newly_created.with_user(self.svc_uid).read(["name"])
