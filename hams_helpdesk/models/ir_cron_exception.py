# Copyright © Bruce Perens K6BP.
# SPDX-License-Identifier: AGPL-3.0-or-later
"""ir.cron hook for automatic server-exception tickets (see exception_fingerprint.py).

``_callback`` runs the job's server action and re-raises after rolling back, and Odoo then only logs
the failure. This reports it and re-raises the same exception, so the job's own failure handling is
unchanged. The mail queue runs as a cron job, so a failure that escapes it is reported here; a
single message that fails to send is recorded on the mail itself by Odoo and is not an exception.
"""

from odoo import models

from .exception_fingerprint import report_exception
from .exception_scrub import scrub_text


class IrCronExceptionReport(models.Model):
    _inherit = "ir.cron"

    # [@ANCHOR: hams_helpdesk:exc_ir_cron_callback]
    def _callback(self, cron_name, server_action_id):
        try:
            return super()._callback(cron_name, server_action_id)
        except Exception as exc:  # audit-ignore-catch-all: re-raised unchanged below; Tested by [@ANCHOR: hams_helpdesk:exc_ir_cron_callback]
            report_exception(
                self.env.registry,
                exc,
                "cron",
                uid=self.env.uid,
                logged_in=True,
                rule="cron: " + scrub_text(cron_name, strict=False),
            )
            raise
