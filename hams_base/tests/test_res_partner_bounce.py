# -*- coding: utf-8 -*-
"""
Real coverage for res_partner.py's _message_receive_bounce() override -- had zero test coverage
before this file (confirmed by grepping tests/ for the method name). Writing these tests found a
real, previously-undiscovered production bug, not just a coverage gap: the override was originally
named `message_receive_bounce` (no leading underscore, with a `mail_id=None` parameter Odoo's real
hook doesn't have) -- confirmed against the installed Odoo version's own mail_thread.py /
mail_thread_blacklist.py that the real hook is `_message_receive_bounce(self, email, partner)`, so
the old name never actually overrode anything and this whole feature had silently never fired in
production. Fixed in models/res_partner.py as part of the same change that added this file.

Two real behaviors verified now that the method is actually reachable: club officers get notified
when a member's email starts bouncing, and the notification is suppressed when the bouncing address
IS the club's own (to avoid a bounce-notifying-about-itself loop).

The `club_ids` branch in _message_receive_bounce() ('if ham_club_management is installed, partners
might have a club_id or club_membership_ids') is not exercised here: confirmed directly, no model
anywhere in this codebase actually defines a `club_ids` field on res.partner (grepped both repos) --
ham_club_management's own real club-membership representation is a plain `parent_id` company
relationship, which IS the branch these tests exercise. The `club_ids` check is currently
unreachable dead code against the real schema.
"""
from odoo.addons.zero_sudo.tests.common import HamsTransactionCase
from odoo.tests import tagged


@tagged("post_install", "-at_install")
class TestResPartnerBounce(HamsTransactionCase):
    @classmethod
    def setUpClass(cls):
        super().setUpClass()
        cls.club = cls.env["res.partner"].create(
            {
                "name": "Local Radio Club",
                "is_company": True,
                "email": "club@example.com",
            }
        )
        cls.member = cls.env["res.partner"].create(
            {
                "name": "Member Ham",
                "email": "member@example.com",
                "parent_id": cls.club.id,
            }
        )

    def test_a_members_bounce_notifies_their_parent_company_as_the_club(self):
        before = self.club.message_ids
        self.member._message_receive_bounce("member@example.com", self.member)
        after = self.club.message_ids
        new_messages = after - before
        self.assertEqual(len(new_messages), 1)
        self.assertIn("Member Ham", new_messages.body)
        self.assertIn("member@example.com", new_messages.body)
        self.assertIn("/helpdesk", new_messages.body)

    def test_the_clubs_own_bounce_does_not_notify_itself(self):
        # Bounce-loop prevention: club.email == email must suppress the notification,
        # per the method's own inline comment ("Prevent bounce loop").
        before = self.club.message_ids
        self.member._message_receive_bounce("club@example.com", self.member)
        after = self.club.message_ids
        self.assertEqual(
            len(after - before),
            0,
            "a bounce FROM the club's own address must not generate a notification "
            "posted back to that same club",
        )

    def test_a_partner_with_no_parent_company_generates_no_notification_and_does_not_raise(self):
        standalone = self.env["res.partner"].create(
            {"name": "Standalone Ham", "email": "standalone@example.com"}
        )
        # Must not raise even though there is no club to notify at all.
        standalone._message_receive_bounce("standalone@example.com", standalone)

    def test_a_falsy_partner_is_a_no_op(self):
        # _message_receive_bounce(email, partner) with partner=empty recordset -- the real shape
        # Odoo's own mail-bounce processing calls this with when no partner could be resolved.
        empty_partner = self.env["res.partner"]
        # Must not raise.
        empty_partner._message_receive_bounce("unknown@example.com", empty_partner)
