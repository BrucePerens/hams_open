# -*- coding: utf-8 -*-
from odoo import models, api
import logging

_logger = logging.getLogger(__name__)

class MailThread(models.AbstractModel):
    _inherit = 'mail.thread'

    @api.model
    def message_route(self, message, message_dict, model=None, thread_id=None, custom_values=None):
        """
        Intercept routing to prevent non-bounce emails sent to not-read@hams.com
        from cluttering up the system or creating unexpected records.
        """
        # We also want to intercept auto-mail-failure if it's hitting standard routes
        bounce_alias = self.env['zero_sudo.security.utils']._get_system_param('mail.bounce.alias') or 'auto-mail-failure'
        not_read_alias = 'not-read'
        
        to_emails = message_dict.get('to', '').lower()
        subject = message_dict.get('subject', '').lower()
        body = message_dict.get('body', '').lower()

        is_bounce_route = bounce_alias in to_emails
        is_not_read_route = not_read_alias in to_emails

        if is_bounce_route or is_not_read_route:
            # Check for unsubscribe intent
            if 'unsubscribe' in subject or 'unsubscribe' in body:
                _logger.info("Recognized manual unsubscribe request from %s", message_dict.get('email_from'))
                # We could auto-unsubscribe them here, or send them an email pointing to the unsubscribe page.
                # For now, we will log it and drop it so it doesn't create garbage tickets.
                return []
            
            # Check for Vacation replies / OOO
            if 'out of office' in subject or 'vacation' in subject or 'auto-reply' in subject:
                _logger.info("Dropping vacation reply sent to automated alias: %s", subject)
                return []
            
            # Check if it's an actual bounce (DSN)
            if message_dict.get('bounced_email') or message_dict.get('bounced_partner'):
                # It's a recognized bounce, let Odoo process it
                pass
            elif is_not_read_route:
                # It was sent to not-read but is not a bounce and not an unsubscribe
                _logger.info("Dropping garbage/reply sent to not-read alias: %s", subject)
                return []
        
        return super().message_route(message, message_dict, model, thread_id, custom_values)
