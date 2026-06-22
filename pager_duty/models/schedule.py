# -*- coding: utf-8 -*-
import logging
from odoo import models, fields, api

_logger = logging.getLogger(__name__)

class PagerSchedule(models.Model):
    """
    Extends calendar.event to support pager duty scheduling.
    This model is multi-tenant as it inherits from calendar.event, and we add
    website_id to further isolate shifts by website.
    """
    _inherit = "calendar.event"

    is_pager_duty = fields.Boolean(string="Is Pager Duty Shift", default=False, index=True)
    website_id = fields.Many2one("website", string="Website", ondelete="cascade", index=True)

    @api.model
    def get_current_on_duty_admin(self):
        """
        Returns the user currently on duty for the active website.
        """
        # [@ANCHOR: test_pager_notification]
        now = fields.Datetime.now()
        domain = [
            ("is_pager_duty", "=", True),
            ("start", "<=", now),
            ("stop", ">=", now),
        ]

        # Enforce strict schema contract. We do not mask missing dependencies.
        if self.env.context.get("website_id"):
            domain += ["|", ("website_id", "=", False), ("website_id", "=", self.env.context.get("website_id"))]
        else:
            current_website = self.env["website"].get_current_website()
            if current_website:
                domain += ["|", ("website_id", "=", False), ("website_id", "=", current_website.id)]

        event = self.env["calendar.event"].search(domain, limit=1)
        if event and event.user_id:
            return event.user_id
        return False
