# -*- coding: utf-8 -*-
# Copyright © Bruce Perens K6BP. License: AGPL-3.0.
from odoo import models


# [@ANCHOR: hams_base_mail_notification_mail_notification]
class MailNotification(models.Model):
    _inherit = "mail.notification"

    # [@ANCHOR: hams_base:notification_gc_off]
    # Verified by [@ANCHOR: hams_base:test_notification_gc_off]
    def _gc_notifications(self, max_age_days=180):
        """Core's daily "Notification: Delete Notifications older than 6 Month" cron deleted read notifications. They
        are log records and log records are permanent (Bruce, 2026-11-20). Returns what core returns for nothing left."""
        return 0, False
