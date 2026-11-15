# -*- coding: utf-8 -*-
"""Core Odoo checks mail.blacklist only for chatter notifications. A mail.mail made directly (a template
send_mail, the weekly digest, onboarding and consent mail) goes to every address, including one SES reported as a
permanent bounce or a spam complaint (ses_webhook adds those to mail.blacklist). This drops blacklisted addresses
from a queued mail just before it is sent and marks the mail `exception` (the state a refused send gets, so callers that check it are told) when no recipient is left."""
from odoo import models
from odoo.tools import email_normalize, email_split


# [@ANCHOR: hams_base:mail_skips_blacklisted_recipients]
class MailMailBlacklist(models.Model):
    _inherit = "mail.mail"

    # [@ANCHOR: hams_base:mail_send_filters_outgoing]
    # Verified by [@ANCHOR: test_send_filters_only_outgoing_mail]
    # Verified by [@ANCHOR: test_a_blacklisted_recipient_is_dropped_before_send]
    def _send(self, *args, **kwargs):
        outgoing = self.filtered(lambda m: m.state == "outgoing")
        if outgoing:
            outgoing._drop_blacklisted_recipients()
        return super()._send(*args, **kwargs)

    def _drop_blacklisted_recipients(self):
        wanted = set()
        for mail in self:
            wanted.update(email_normalize(a) for a in email_split(f"{mail.email_to or ''},{mail.email_cc or ''}"))
            wanted.update(p.email_normalized for p in mail.recipient_ids)
        wanted.discard(None)
        wanted.discard(False)
        if not wanted:
            return
        svc_uid = self.env["zero_sudo.security.utils"]._get_service_uid("zero_sudo.mail_service_internal")
        blocked = set(
            self.env["mail.blacklist"].with_user(svc_uid).search([("email", "in", list(wanted)), ("active", "=", True)]).mapped("email")
        )
        if not blocked:
            return
        for mail in self:
            def keep(address):
                return email_normalize(address) not in blocked

            to_kept = [a for a in email_split(mail.email_to or "") if keep(a)]
            cc_kept = [a for a in email_split(mail.email_cc or "") if keep(a)]
            partners_kept = mail.recipient_ids.filtered(lambda p: p.email_normalized not in blocked)
            if not (to_kept or cc_kept or partners_kept):
                if mail.email_to or mail.email_cc or mail.recipient_ids:
                    mail.write({"state": "exception", "failure_reason": "Every recipient is on the mail blacklist."})
                continue
            mail.write(
                {
                    "email_to": ", ".join(to_kept),
                    "email_cc": ", ".join(cc_kept),
                    "recipient_ids": [(6, 0, partners_kept.ids)],
                }
            )
