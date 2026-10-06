# Copyright © Bruce Perens K6BP.
# SPDX-License-Identifier: AGPL-3.0-or-later
"""Inbound mail inspection at the one place every mail passes: mail.thread.message_parse().

Odoo's parse keeps only one of a multipart/alternative's parts and flattens headers, so the checks that
need the whole message (header allow-list, text/plain against text/html, attachment names) must run
here, on the parsed `email.message.Message`. The findings are parked on the database cursor, keyed by
Message-ID, for the one place that knows the target record: UntrustedMixin.message_post(). The cursor
cache is cleared on commit and rollback, so nothing outlives the transaction that parsed the mail.
See docs/security/TICKET_PROMPT_INJECTION.md.
"""

from odoo import api, models

from . import untrusted_text as ut

MAIL_STASH_KEY = "hams_helpdesk_untrusted_mail"


def stash_for(env):
    return env.cr.cache.setdefault(MAIL_STASH_KEY, {})


class MailThread(models.AbstractModel):
    _inherit = "mail.thread"

    @api.model
    def message_parse(self, message, save_original=False):
        msg_dict = super().message_parse(message, save_original=save_original)
        result = ut.inspect_mail(message)  # never raises: an error is itself a finding
        key = (msg_dict or {}).get("message_id") or ""
        stash_for(self.env)[key] = {
            "findings": result.findings,
            "score": result.score,
            "removed": result.removed,
        }
        return msg_dict
