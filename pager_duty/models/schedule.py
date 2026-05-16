# -*- coding: utf-8 -*-
from odoo import models, fields, api


class PagerSchedule(models.Model):
    _inherit = "calendar.event"

    is_pager_duty = fields.Boolean(string="Is Pager Duty Shift", default=False)

    @api.model
    def get_current_on_duty_admin(self):
        # [@ANCHOR: test_pager_notification]
        now = fields.Datetime.now()
        domain = [
            ("is_pager_duty", "=", True),
            ("start", "<=", now),
            ("stop", ">=", now),
        ]
        event = self.env["calendar.event"].search(domain, limit=1)
        if event and event.user_id:
            return event.user_id
        return False
