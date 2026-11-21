# -*- coding: utf-8 -*-
# Copyright © Bruce Perens K6BP. License: AGPL-3.0.
from odoo.addons.zero_sudo.tests.common import HamsTransactionCase
from odoo.tests import tagged


@tagged("post_install", "-at_install")
class TestNotificationGcOff(HamsTransactionCase):
    # [@ANCHOR: hams_base:test_notification_gc_off]
    def test_old_read_notification_survives_the_gc(self):
        # Tests [@ANCHOR: hams_base_mail_notification_mail_notification]
        # Tests [@ANCHOR: hams_base:notification_gc_off]
        partner = self.env["res.partner"].create({"name": "Notification Gc Off"})
        message = self.env["mail.message"].create({"model": "res.partner", "res_id": partner.id, "body": "x", "message_type": "comment"})
        note = self.env["mail.notification"].create({
            "mail_message_id": message.id, "res_partner_id": partner.id, "notification_type": "inbox",
            "notification_status": "sent", "is_read": True, "read_date": "2020-01-01 00:00:00",
        })
        self.assertEqual(self.env["mail.notification"]._gc_notifications(), (0, False))
        self.assertTrue(note.exists(), "[!] DIAGNOSTIC FOR AI: notifications are logs and must never be purged.")
