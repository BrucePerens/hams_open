# This software is distributed under the terms of the Affero General Public License (AGPL-3).

from odoo import models, fields


class CalendarEvent(models.Model):
    _inherit = "calendar.event"

    helpdesk_ticket_ids = fields.One2many("hams_helpdesk.ticket", "calendar_event_id", string="Helpdesk Tickets")

    def get_current_on_duty_admin(self):
        """
        Base implementation for on-duty admin resolution.
        Overridden by pager_duty module.
        """
        return False

    def get_upcoming_duty_shifts(self):
        return self.env["calendar.event"].browse()
