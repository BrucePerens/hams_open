from odoo import models
from odoo.tools.translate import _
# -*- coding: utf-8 -*-
# from odoo import models, api, _
import html
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
                        # Adversarial security review, 2026-09-03: user.name/
                        # old_email/new_email interpolated raw into HTML sent
                        # via mail.mail directly (not through message_post's
                        # own chatter-sanitization pipeline). Low real value
                        # to an attacker (only reaches the account's own old
                        # email address, a self-XSS shape), but escaped for
                        # the same defense-in-depth reason as the bounce
                        # notification fix above.
                        mail_values = {
                            'subject': _('Security Alert: Your HAMS Email Address was Changed'),
                            'body_html': _(
                                '<p>Hello %s,</p>'
                                '<p>This is an automated security notification from HAMS.</p>'
                                '<p>Your account email address has just been changed from <strong>%s</strong> to <strong>%s</strong>.</p>'
                                '<p>If you made this change, no further action is required.</p>'
                                '<p style="color: red;"><strong>If you did not authorize this change, please contact admin@hams.com immediately.</strong></p>'
                            ) % (
                                html.escape(user.name or ''),
                                html.escape(old_email or ''),
                                html.escape(new_email or ''),
                            ),
                            'email_to': old_email,
                            'email_from': self.env.company.catchall_formatted or self.env.company.email_formatted or 'admin@hams.com',
                        }
                        # Use the facility service account for mailing
                        svc_uid = self.env['zero_sudo.security.utils']._get_service_uid('zero_sudo.odoo_facility_service_internal')
                        mail = self.env['mail.mail'].with_user(svc_uid).create(mail_values)
                        mail.send()
                    except (KeyError, ValueError) as e:  # audit-ignore-catch-all
                        _logger.exception("Failed to send security warning to old email %s: %s", old_email, e)

        return res
