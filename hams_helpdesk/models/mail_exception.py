# Copyright © Bruce Perens K6BP.
# SPDX-License-Identifier: AGPL-3.0-or-later
"""Mail hooks for automatic server-exception tickets (see exception_fingerprint.py).

Odoo does not raise when one outgoing message fails: ``mail.mail._send`` catches the error, sets
``state = 'exception'`` with a ``failure_type`` and ``failure_reason`` on the mail, logs it and
carries on, so the mail-queue cron job never fails and nobody is told. Every one of those paths (and
the failed SMTP connection in ``send()``) ends in ``_postprocess_sent_message`` with a
``failure_type``, which is where this reports.

What reaches a ticket is counts and fixed labels only: the failure type, a class label chosen from a
fixed vocabulary by ``mail_failure_class``, the mail server id and how many messages were in the
batch. The failure text, recipients, subject and body are not read into the report at all.

A failure while processing an incoming bounce is reported from ``_routing_handle_bounce`` and
re-raised unchanged; that report goes through the normal exception path, so it carries the exception
class and frames but never the message.
"""

from odoo import models

from . import exception_scrub as scrub
from .exception_fingerprint import report_event, report_exception

MAIL_MODULE = "mail"
MAIL_ROUTE = "mail.mail/send"


# [@ANCHOR: hams_helpdesk:exc_report_mail_failure]
def report_mail_failure(registry, failure_type, reason, server_id, count):
    """Reports a failed outgoing message batch. ``reason`` is only classified, never stored."""
    label = scrub.mail_failure_class(reason)
    transport = f"smtp:server {server_id}" if server_id else "smtp:default"
    frame = f"{failure_type}/{label} via {transport}"
    trace = "\n".join(
        (
            "Outgoing mail could not be sent.",
            f"failure_type: {failure_type}",
            f"failure_class: {label}",
            f"transport: {transport}",
            f"messages in this batch: {int(count)}",
            "Recipients, subjects, bodies and the server's own message are never recorded.",
        )
    )
    return report_event(
        registry,
        "mail",
        "mail.mail.send_failure",
        MAIL_MODULE,
        frame,
        MAIL_ROUTE,
        trace,
        f"Mail send failure ({failure_type}/{label}) via {transport}",
    )


class MailMailExceptionReport(models.Model):
    _inherit = "mail.mail"

    # [@ANCHOR: hams_helpdesk:exc_mail_postprocess]
    def _postprocess_sent_message(self, success_pids, success_emails, failure_reason=False, failure_type=None):
        # Read before super(): a failed mail is kept, but the batch must not be touched afterwards.
        count = len(self)
        server_id = self[:1].mail_server_id.id
        result = super()._postprocess_sent_message(
            success_pids, success_emails, failure_reason=failure_reason, failure_type=failure_type
        )
        if failure_type and failure_type not in scrub.MAIL_FAILURES_NOT_REPORTED:
            report_mail_failure(self.env.registry, failure_type, failure_reason, server_id, count)
        return result


class MailThreadBounceReport(models.AbstractModel):
    _inherit = "mail.thread"

    # [@ANCHOR: hams_helpdesk:exc_mail_bounce]
    def _routing_handle_bounce(self, email_message, message_dict):
        try:
            return super()._routing_handle_bounce(email_message, message_dict)
        except Exception as exc:  # audit-ignore-catch-all: re-raised unchanged below; Tested by [@ANCHOR: hams_helpdesk:exc_mail_bounce]
            report_exception(
                self.env.registry,
                exc,
                "mail",
                uid=self.env.uid,
                logged_in=True,
                path="mail.thread/_routing_handle_bounce",
            )
            raise
