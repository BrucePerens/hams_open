# -*- coding: utf-8 -*-
# Copyright © Bruce Perens K6BP.
# SPDX-License-Identifier: AGPL-3.0-or-later
"""Moderation appeals are kept when the suspended user or group is deleted outright (Bruce, 2026-11-16)."""
from odoo.exceptions import UserError
from odoo.tests.common import tagged
from odoo.addons.zero_sudo.tests.common import HamsTransactionCase


@tagged("post_install", "-at_install")
class TestAppealSurvivesDelete(HamsTransactionCase):
    # [@ANCHOR: user_websites:test_appeal_survives_user_delete]
    def test_appeal_survives_user_delete(self):
        # Tests [@ANCHOR: user_websites:appeal_compute_target_label]
        user = self.env["res.users"].create({"name": "Appeal Gone", "login": "appeal_gone_member"})
        appeal = self.env["content.violation.appeal"].create({"user_id": user.id, "reason": "r"})
        self.assertEqual(appeal.target_label, "Appeal Gone")
        user.unlink()
        appeal.invalidate_recordset()
        self.assertTrue(appeal.exists(), "the moderation history is kept")
        self.assertFalse(appeal.user_id)
        self.assertEqual(appeal.target_label, "Appeal Gone", "the orphaned appeal is still identifiable")
        with self.assertRaises(UserError):
            appeal.action_approve()
        self.assertEqual(appeal.state, "new")
        appeal.action_reject()
        self.assertEqual(appeal.state, "rejected")

    def test_appeal_survives_group_delete(self):
        # Tests [@ANCHOR: user_websites:appeal_compute_target_label]
        group = self.env["user.websites.group"].create({"name": "Appeal Gone Group", "website_slug": "appeal-gone-group"})
        appeal = self.env["content.violation.appeal"].create({"group_id": group.id, "reason": "r"})
        group.unlink()
        appeal.invalidate_recordset()
        self.assertTrue(appeal.exists())
        self.assertFalse(appeal.group_id)
        self.assertEqual(appeal.target_label, "Appeal Gone Group")
