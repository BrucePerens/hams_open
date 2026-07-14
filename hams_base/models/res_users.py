# -*- coding: utf-8 -*-
from odoo import models, api, _
import logging

_logger = logging.getLogger(__name__)

class ResUsers(models.Model):
    _inherit = "res.users"

    def write(self, vals):
        """
        Override write to detect email/login changes and notify the old address.
        """
        # Pre-capture old emails for users being modified
        old_emails = {}
        if 'email' in vals or 'login' in vals:
            for user in self:
                old_emails[user.id] = user.email or user.login

        res = super().write(vals)

        # Process notifications after the write is successful
        if 'email' in vals or 'login' in vals:
            for user in self:
                old_email = old_emails.get(user.id)
                new_email = user.email or user.login
                
                if old_email and new_email and old_email.lower() != new_email.lower():
                    # Send an email to the OLD address warning them of the change
                    try:
                        mail_values = {
                            'subject': _('Security Alert: Your HAMS Email Address was Changed'),
                            'body_html': _(
                                '<p>Hello %s,</p>'
                                '<p>This is an automated security notification from HAMS.</p>'
                                '<p>Your account email address has just been changed from <strong>%s</strong> to <strong>%s</strong>.</p>'
                                '<p>If you made this change, no further action is required.</p>'
                                '<p style="color: red;"><strong>If you did not authorize this change, please contact admin@hams.com immediately.</strong></p>'
                            ) % (user.name, old_email, new_email),
                            'email_to': old_email,
                            'email_from': self.env.company.catchall_formatted or self.env.company.email_formatted or 'admin@hams.com',
                        }
                        mail = self.env['mail.mail'].sudo().create(mail_values)
                        mail.send()
                    except Exception as e:
                        _logger.error("Failed to send security warning to old email %s: %s", old_email, e)

        return res
