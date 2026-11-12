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
from .inbound_spam_filter import mail_signals, ses_verdicts
from odoo.addons.zero_sudo import public_rpc

MAIL_STASH_KEY = "hams_helpdesk_untrusted_mail"
# [@ANCHOR: hams_helpdesk:inbound_reply_message_models]
# The records an inbound message becomes (a ticket, or a pager incident through the pager adapter): a Message-ID stored on one of
# them is mail hams.com sent or received.
REPLY_MESSAGE_MODELS = ["hams_helpdesk.ticket", "pager.incident"]


# [@ANCHOR: hams_helpdesk:inbound_stash_for]
def stash_for(env):
    return env.cr.cache.setdefault(MAIL_STASH_KEY, {})


class MailThread(models.AbstractModel):
    _inherit = "mail.thread"

    # [@ANCHOR: hams_helpdesk:inbound_message_parse]
    @public_rpc
    @api.model
    def message_parse(self, message, save_original=False):
        msg_dict = super().message_parse(message, save_original=save_original)
        result = ut.inspect_mail(message)  # never raises: an error is itself a finding
        key = (msg_dict or {}).get("message_id") or ""
        stash_for(self.env)[key] = {
            "findings": result.findings,
            "score": result.score,
            "removed": result.removed,
            # Amazon SES's scan verdicts, for message_new(); not a key of msg_dict, because Odoo passes every key of msg_dict on to
            # mail.message.create() and an unknown one is refused.
            "ses_verdicts": ses_verdicts(message),
            # Header facts and HTML shape of the raw message (List-Unsubscribe, Precedence, SES authentication results, text/plain
            # against text/html, tables, images, tracking pixels) for the bulk-mail rules of inbound_spam_reasons(): the sanitised
            # ticket body has lost all of them. Read by helpdesk_ticket.message_new() and pager.incident.message_new().
            "mail_signals": self._with_verified_reply_headers(mail_signals(message)),
        }
        return msg_dict

    @api.model
    # [@ANCHOR: hams_helpdesk:inbound_verified_reply_headers]
    def _with_verified_reply_headers(self, signals):
        """``signals`` (from ``mail_signals``) with ``reply_headers`` set: True only when one of its
        ``referenced_message_ids`` is the Message-ID of a message already stored on a ticket or a pager incident. The sender writes
        In-Reply-To and References, so a header alone must not exempt a message from the bulk-mail rules. The lookup
        runs as the helpdesk service account (the mail-ingest identity cannot read tickets)."""
        ids = signals.get("referenced_message_ids") or []
        known = False
        if ids:
            hd_env = self.env["zero_sudo.security.utils"]._get_service_env("hams_helpdesk.user_helpdesk_service")
            known = bool(
                hd_env["mail.message"].search_count([("model", "in", REPLY_MESSAGE_MODELS), ("message_id", "in", ids)])
            )
        signals["reply_headers"] = known
        return signals
