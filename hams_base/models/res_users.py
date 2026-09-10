from odoo import models
from odoo.tools.translate import _
# -*- coding: utf-8 -*-
# from odoo import models, api, _
import html
import logging

_logger = logging.getLogger(__name__)

class ResUsers(models.Model):
    _inherit = "res.users"

    # [@ANCHOR: hams_base:COMM_res_users_write]
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
                        # bug-hunt (2026-09-09): the whole block below runs
                        # inside `self.env.cr.savepoint()`, not just a bare
                        # try -- load-bearing, not decorative. This is a
                        # `for user in self:` loop: `_get_service_uid()`'s
                        # own SQL-backed uid lookup does a real Postgres
                        # `RAISE EXCEPTION` on a missing/disabled/non-service
                        # account (zero_sudo_get_service_uid() in
                        # zero_sudo/data/postgres_procedures.xml), which
                        # aborts the CURRENT transaction. Before this fix,
                        # `except Exception` alone would catch that for THIS
                        # user, but leave the transaction poisoned -- the
                        # NEXT user's own `mail.mail.create(...)` call (a
                        # real SQL INSERT) would then fail with
                        # `InFailedSqlTransaction`, cascading a single
                        # account's misconfiguration into every subsequent
                        # user in the same batch `write()` failing too, each
                        # logged as if IT had its own independent mail
                        # failure. The savepoint isolates each user's own
                        # attempt so one failure can't poison the next.
                        with self.env.cr.savepoint():
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
                    except Exception as e:  # audit-ignore-catch-all
                        # bug-hunt (2026-09-09): mail.mail.send() can raise
                        # a variety of delivery exceptions (SMTP errors,
                        # socket/connection failures, Odoo's own
                        # MailDeliveryException) -- none of which are
                        # KeyError/ValueError -- and _get_service_uid() can
                        # raise AccessError. A real send failure here used
                        # to propagate uncaught out of write(), meaning a
                        # transient mail-server outage would abort an
                        # otherwise-successful email/login change instead of
                        # just skipping the best-effort security notice.
                        # Class 20.
                        _logger.exception("Failed to send security warning to old email %s: %s", old_email, e)

        return res
