# -*- coding: utf-8 -*-
from odoo.tests.common import tagged
from odoo.addons.zero_sudo.tests.common import HamsTransactionCase
from odoo import fields
from datetime import timedelta


@tagged("post_install", "-at_install")
class TestScheduleEdgeCases(HamsTransactionCase):
    def test_01_empty_schedule(self):
        """Verify get_current_on_duty_admin returns False when no shifts exist."""
        self.env["calendar.event"].search([]).unlink()
        admin = self.env["calendar.event"].get_current_on_duty_admin()
        self.assertFalse(
            admin, "MUST return False when the schedule is completely empty."
        )

    def test_02_overlapping_shifts(self):
        """Verify the method safely returns exactly one admin when shifts overlap."""
        self.env["calendar.event"].search([]).unlink()
        user1 = self.env["res.users"].create({"name": "User 1", "login": "u1"})
        user2 = self.env["res.users"].create({"name": "User 2", "login": "u2"})
        now = fields.Datetime.now()

        self.env["calendar.event"].create(
            [
                {
                    "name": "Shift 1",
                    "start": now - timedelta(hours=1),
                    "stop": now + timedelta(hours=1),
                    "is_pager_duty": True,
                    "user_id": user1.id,
                },
                {
                    "name": "Shift 2",
                    "start": now - timedelta(hours=1),
                    "stop": now + timedelta(hours=1),
                    "is_pager_duty": True,
                    "user_id": user2.id,
                },
            ]
        )

        admin = self.env["calendar.event"].get_current_on_duty_admin()
        self.assertTrue(admin, "An admin MUST be returned.")
        self.assertIn(
            admin.id,
            [user1.id, user2.id],
            "MUST safely return one of the overlapping users without crashing.",
        )
