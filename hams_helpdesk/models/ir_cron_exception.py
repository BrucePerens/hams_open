# Copyright © Bruce Perens K6BP.
# SPDX-License-Identifier: AGPL-3.0-or-later
"""ir.cron hook for automatic server-exception tickets (see exception_fingerprint.py).

``_callback`` runs the job's server action and re-raises after rolling back, and Odoo then only logs
the failure. This reports it and re-raises the same exception, so the job's own failure handling is
unchanged. The mail queue runs as a cron job, so a failure that escapes it is reported here; a
single message that fails to send is recorded on the mail itself by Odoo and is not an exception.
"""

import logging
import threading

from odoo import models

from .exception_fingerprint import report_exception
from .exception_scrub import scrub_text


MAX_LOGGED_PER_RUN = 5


# [@ANCHOR: hams_helpdesk:exc_cron_logged_failures]
class _LoggedFailures(logging.Handler):
    """Keeps the exceptions that code running on this thread logged with ``exc_info`` at ERROR or above.

    A cron method that catches a failure per record and only calls ``_logger.exception`` finishes normally, so
    ``_callback`` would never see it; a record that fails the same way every run was then invisible."""

    # [@ANCHOR: hams_helpdesk:exc_cron_logged_failures_init]
    def __init__(self):
        super().__init__(level=logging.ERROR)
        self.thread = threading.get_ident()
        self.caught = []

    # [@ANCHOR: hams_helpdesk:exc_cron_logged_failures_emit]
    def emit(self, record):
        if record.thread == self.thread and record.exc_info and record.exc_info[1] is not None:
            self.caught.append(record.exc_info[1])


# [@ANCHOR: hams_helpdesk_ir_cron_exception_ir_cron_exception_report]
class IrCronExceptionReport(models.Model):
    _inherit = "ir.cron"

    # [@ANCHOR: hams_helpdesk:exc_ir_cron_callback]
    def _callback(self, cron_name, server_action_id):
        logged = _LoggedFailures()
        root = logging.getLogger()
        root.addHandler(logged)
        rule = "cron: " + scrub_text(cron_name, strict=False)
        try:
            result = super()._callback(cron_name, server_action_id)
        except Exception as exc:  # audit-ignore-catch-all: re-raised unchanged below; Tested by [@ANCHOR: hams_helpdesk:exc_ir_cron_callback]
            report_exception(
                self.env.registry,
                exc,
                "cron",
                uid=self.env.uid,
                logged_in=True,
                rule=rule,
            )
            raise
        finally:
            root.removeHandler(logged)
        # The job finished, but it logged failures it handled itself (one bad record, retried next run). Report
        # them so a record that never succeeds becomes a ticket; the fingerprint folds repeats into one.
        for exc in logged.caught[:MAX_LOGGED_PER_RUN]:
            report_exception(self.env.registry, exc, "cron", uid=self.env.uid, logged_in=True, rule=rule)
        return result
