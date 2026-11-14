# Copyright © Bruce Perens K6BP.
# SPDX-License-Identifier: AGPL-3.0-or-later
"""A mail that could not be sent raises a pager incident for the operator.

Odoo marks a refused mail `exception` and carries on, and the password reset form answers the same for
members and strangers, so nobody sees a refusal. Bruce decided (2026-11-13) that the operator is told.
The report goes through `pager.incident.report_incident`, which keeps ONE open incident per source:
a mail outage pages once, not once per user. The text is fixed; no address, subject, token or server
message is ever put in it.
"""
import logging

from odoo import models

from odoo.addons.hams_helpdesk.models import exception_scrub as scrub
from odoo.addons.hams_helpdesk.models.exception_fingerprint import _service_env, _without_request

_logger = logging.getLogger(__name__)

MAIL_FAILURE_SOURCE = "mail_send_failure"
MAIL_FAILURE_TEXT = (
    "The site could not send an email (the outgoing mail server refused it or could not be reached). "
    "Password resets, sign-up, consent and invitation mails may not be arriving. Check the outgoing mail "
    "server (for example SES) and the mail.mail records in state 'exception'. No address is recorded here."
)


# [@ANCHOR: pager_duty:page_mail_failure]
def page_mail_failure(registry, failure_type):
    """Reports a failed send as one pager incident. Never raises: a broken alarm must not break the mail queue.
    Runs on its own cursor so a caller's rolled-back savepoint (core rolls the failed mail back) cannot lose it."""
    try:
        with _without_request(), registry.cursor() as cr:
            env = _service_env(cr)
            env["pager.incident"].report_incident(
                {
                    "source": MAIL_FAILURE_SOURCE,
                    "severity": "high",
                    "description": f"{MAIL_FAILURE_TEXT} (failure type: {failure_type})",
                }
            )
    except Exception:  # audit-ignore-catch-all: an alarm failure is logged and swallowed; Tested by [@ANCHOR: pager_duty:page_mail_failure]
        _logger.warning("Could not raise the mail-failure incident.", exc_info=True)


# [@ANCHOR: pager_duty:mail_failure_pages_operator]
class MailMailPagesOperator(models.Model):
    _inherit = "mail.mail"

    # [@ANCHOR: pager_duty:mail_postprocess_sent_message]
    # Verified by [@ANCHOR: test_a_refused_send_raises_one_alarm_with_no_address]
    def _postprocess_sent_message(self, success_pids, success_emails, failure_reason=False, failure_type=None):
        result = super()._postprocess_sent_message(
            success_pids, success_emails, failure_reason=failure_reason, failure_type=failure_type
        )
        # A bad recipient address is that person's data, not a broken mail system (same list the ticket reporter uses).
        if failure_type and failure_type not in scrub.MAIL_FAILURES_NOT_REPORTED:
            page_mail_failure(self.env.registry, failure_type)
        return result
