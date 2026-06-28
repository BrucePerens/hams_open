from odoo import models


class CalendarEvent(models.Model):
    _inherit = "calendar.event"

    def get_current_on_duty_admin(self):
        """
        Base implementation for on-duty admin resolution.
        Overridden by pager_duty module.
        """
        return False
