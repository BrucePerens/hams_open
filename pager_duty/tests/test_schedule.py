# -*- coding: utf-8 -*-
from odoo.addons.hams_test.tests.real_transaction import HamsTransactionCase
from odoo import fields
from datetime import timedelta


class TestPagerSchedule(HamsTransactionCase):
    def setUp(self):
        super(TestPagerSchedule, self).setUp()
        self.calendar_model = self.env["calendar.event"]
        self.test_user = self.env["res.users"].create(
            {
                "name": "Test Duty Admin",
                "login": "duty_admin_test",
            }
        )

    def test_01_get_current_on_duty_admin_active(self):
        now = fields.Datetime.now()
        self.calendar_model.create(
            {
                "name": "Active Pager Shift",
                "start": now - timedelta(hours=1),
                "stop": now + timedelta(hours=1),
                "is_pager_duty": True,
                "user_id": self.test_user.id,
            }
        )

        on_duty = self.calendar_model.get_current_on_duty_admin()
        self.assertEqual(
            on_duty.id,
            self.test_user.id,
            "Scheduling engine failed to return the currently active admin.",
        )

    def test_02_get_current_on_duty_admin_expired(self):
        now = fields.Datetime.now()
        self.calendar_model.create(
            {
                "name": "Expired Pager Shift",
                "start": now - timedelta(hours=4),
                "stop": now - timedelta(hours=2),
                "is_pager_duty": True,
                "user_id": self.test_user.id,
            }
        )

        on_duty = self.calendar_model.get_current_on_duty_admin()
        self.assertFalse(
            on_duty, "Scheduling engine returned an admin for an expired shift."
        )

    def test_03_get_current_on_duty_admin_non_pager(self):
        now = fields.Datetime.now()
        self.calendar_model.create(
            {
                "name": "Regular Meeting",
                "start": now - timedelta(hours=1),
                "stop": now + timedelta(hours=1),
                "is_pager_duty": False,
                "user_id": self.test_user.id,
            }
        )

        on_duty = self.calendar_model.get_current_on_duty_admin()
        self.assertFalse(
            on_duty,
            "Scheduling engine returned an admin for a non-pager-duty calendar event.",
        )
