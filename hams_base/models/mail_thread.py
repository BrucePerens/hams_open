# -*- coding: utf-8 -*-
from odoo import models, api
import email.utils
import logging

_logger = logging.getLogger(__name__)

class MailThread(models.AbstractModel):
    _inherit = 'mail.thread'

    # [@ANCHOR: hams_base:COMM_message_route]
    @api.model
    def message_route(self, message, message_dict, model=None, thread_id=None, custom_values=None):
        """
        Intercept routing to prevent non-bounce emails sent to not-read@hams.com
        from cluttering up the system or creating unexpected records.
        """
        # We also want to intercept auto-mail-failure if it's hitting standard routes
        bounce_alias = self.env['zero_sudo.security.utils']._get_system_param('mail.bounce.alias') or 'auto-mail-failure'
        not_read_alias = 'not-read'
        postmaster_alias = 'postmaster'

        to_emails = message_dict.get('to', '').lower()
        subject = message_dict.get('subject', '').lower()
        body = message_dict.get('body', '').lower()

        # bug-hunt (2026-09-09): `to_emails` is the raw "To" header text,
        # which can legitimately carry multiple comma-separated recipients
        # (each with its own display name). A bare `alias in to_emails`
        # substring test matches any recipient whose LOCAL PART, DOMAIN, OR
        # DISPLAY NAME happens to contain the alias text -- e.g. a real,
        # unrelated address like "postmaster-notify@example.com" or
        # "Do Not Reply <noreply@example.com>" (contains "not-read"'s
        # neighbor "reply", not exact, but the same shape of false-positive
        # risk applies to any alias substring) cc'd alongside hams.com's own
        # mail would silently trigger the drop branches below even though
        # the message was never actually addressed to our own not-read@/
        # postmaster@/bounce alias. Parse the header into individual
        # addresses and match each alias against a recipient's own local
        # part exactly instead of scanning the whole raw header text.
        to_local_parts = {
            addr.split('@', 1)[0]
            for _name, addr in email.utils.getaddresses([to_emails])
            if addr
        }

        def _alias_matches(alias):
            return alias.split('@', 1)[0] in to_local_parts

        is_bounce_route = _alias_matches(bounce_alias)
        is_not_read_route = _alias_matches(not_read_alias)
        # postmaster@ is the RFC 5321-mandated admin contact address for this
        # domain -- it genuinely receives the same DSN-bounce/vacation-reply
        # noise the dedicated bounce alias does, so it gets the same
        # filtering below. Unlike not-read@, anything else sent there is a
        # real inquiry and must NOT be dropped: it falls through to
        # super().message_route(), which resolves the "postmaster" mail.alias
        # (pager_duty/data/mail_alias_data.xml) into a real incident.
        is_postmaster_route = _alias_matches(postmaster_alias)

        if is_bounce_route or is_not_read_route or is_postmaster_route:
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
