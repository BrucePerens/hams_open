# -*- coding: utf-8 -*-
# Copyright © Bruce Perens K6BP.
# SPDX-License-Identifier: AGPL-3.0-or-later
from odoo.tests.common import tagged
from odoo.addons.zero_sudo.tests.common import HamsTransactionCase


@tagged("post_install", "-at_install")
class TestModeratorNamedInNotes(HamsTransactionCase):
    """The notes a moderation action leaves say which moderator did it (the note itself is posted by the mail service)."""

    def setUp(self):
        super().setUp()
        self.moderator = self.env["res.users"].create(
            {
                "name": "Moderator Quixote",
                "login": "moderator_quixote_note",
                "group_ids": [(6, 0, [self.env.ref("user_websites.group_user_websites_administrator").id, self.env.ref("content_moderation.group_content_moderation_moderator").id])],
            }
        )
        self.member = self.env["res.users"].create({"name": "Pardoned Member", "login": "pardoned_member_note"})
        self.group = self.env["user.websites.group"].create({"name": "Pardoned Group", "website_slug": "pardonedgroupnote"})

    def _notes(self, record):
        return " ".join(record.message_ids.mapped("body"))

    def test_pardon_notes_name_the_moderator(self):
        # Tests [@ANCHOR: user_websites:COMM_action_pardon_user_websites]
        # Tests [@ANCHOR: user_websites:COMM_action_pardon_group_websites]
        self.member.with_user(self.moderator).action_pardon_user_websites()
        self.group.with_user(self.moderator).action_pardon_group_websites()
        self.assertIn("Moderator Quixote", self._notes(self.member.partner_id))
        self.assertIn("Moderator Quixote", self._notes(self.group))

    def test_strike_notes_name_the_moderator(self):
        # Tests [@ANCHOR: user_websites:COMM_apply_enforcement_action]
        report = self.env["content.violation.report"].create(
            {"target_url": "/pardonedgroupnote", "description": "x" * 20, "content_group_id": self.group.id}
        )
        report.with_user(self.moderator).action_take_action_and_strike()
        self.assertIn("Moderator Quixote", self._notes(report))
