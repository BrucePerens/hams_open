# Copyright © Bruce Perens K6BP.
# SPDX-License-Identifier: AGPL-3.0-or-later
"""The chatter of a ticket whose triage verdict is phishing shows its message bodies with the links as plain text.

Every message the Odoo web client shows is sent to it through ``mail.message._to_store()``, the one place that puts the body in
the data the browser renders. Overriding it here changes what is shown and nothing that is stored: the message keeps its links
(the shared text filter keeps ``href`` on purpose), and a person's override or a changed verdict shows the right form at once.
Full description: README.md, "Phishing links are defanged for staff".
"""

from odoo import models
from odoo.addons.mail.tools.discuss import Store

from .link_defang import defang_html

_TICKET_MODEL = "hams_helpdesk.ticket"


# [@ANCHOR: hams_helpdesk_mail_message_defanged_mail_message_defanged]
class MailMessageDefanged(models.Model):
    _inherit = "mail.message"

    # [@ANCHOR: hams_helpdesk:message_defanged_ticket_ids]
    def _defanged_ticket_ids(self):
        """The ids of the tickets, among those the messages in self are posted on, whose links are shown as text (verdict
        ``phishing`` and not overridden by a person). One search for the whole recordset, run as the helpdesk service account
        so the answer does not depend on what the reader may see of the ticket. Returns a set of ints, empty when none of the
        messages is on a ticket."""
        ids = {m.res_id for m in self if m.model == _TICKET_MODEL and m.res_id}
        if not ids:
            return set()
        hd_env = self.env["zero_sudo.security.utils"]._get_service_env("hams_helpdesk.user_helpdesk_service")
        return set(
            hd_env[_TICKET_MODEL]
            .search([("id", "in", list(ids)), ("triage_verdict", "=", "phishing"), ("triage_overridden", "=", False)])
            .ids
        )

    # [@ANCHOR: hams_helpdesk:message_to_store_defanged]
    def _to_store(self, store, fields, **kwargs):
        """Odoo's ``_to_store()`` with one change: when ``fields`` asks for the message ``body`` and some of the messages are on
        a ticket whose links are shown as text, the body of those messages is sent defanged (``defang_html``). Every other
        message, and every other field, is exactly Odoo's. The stored body is not touched. ``fields`` is the caller's list of
        field names; a copy is changed, never the caller's list."""
        if isinstance(fields, (list, tuple)) and "body" in fields:
            defanged = self._defanged_ticket_ids()
            if defanged:
                fields = [
                    Store.Attr(
                        "body",
                        value=lambda m, _ids=defanged: defang_html(m.body)
                        if m.model == _TICKET_MODEL and m.res_id in _ids
                        else m.body,
                    )
                    if name == "body"
                    else name
                    for name in fields
                ]
        return super()._to_store(store, fields, **kwargs)
