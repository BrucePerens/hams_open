# Copyright © Bruce Perens K6BP.
# SPDX-License-Identifier: AGPL-3.0-or-later
"""Entry point for failures of external daemons (see exception_fingerprint.py).

Daemons (RabbitMQ consumers, sync and ingest programs, the Rust daemons) run outside Odoo under
systemd, so no Odoo hook sees them fail. Two things report: the systemd ``OnFailure=`` template unit
``hams-exception-report@.service`` (the unit failed and was not restarted successfully) and the
``reporting_failures`` helper a Python daemon wraps its loop in (an uncaught exception). Both call
``report_daemon_failure`` over the JSON-2 API with the daemon key the Daemon Key Manager issues to the
exception reporter service account; see ``daemon/README.md``.

The method accepts only that account, takes a unit name, a kind, a result word and a journal tail,
validates each against a fixed shape, scrubs the tail, and goes through the same fingerprint, rate
limit and own-cursor writer as every other report.
"""

from odoo import api, models
from odoo.exceptions import AccessError, UserError

from . import exception_scrub as scrub
from .exception_fingerprint import SERVICE_XMLID, report_event


class ExceptionFingerprintDaemon(models.Model):
    _inherit = "hams_helpdesk.exception.fingerprint"

    # [@ANCHOR: hams_helpdesk:exc_report_daemon_failure]
    @api.model
    def report_daemon_failure(self, unit, kind="unit_failed", result="", journal_tail=""):
        """Reports a failed daemon. Returns {"recorded": bool}; False means rate limited or de-duplicated.

        Only the exception reporter service account may call it. ``unit`` is the systemd unit name,
        ``kind`` one of ``scrub.DAEMON_KINDS``, ``result`` systemd's Result word (anything else is
        stored as "unknown") and ``journal_tail`` free text that is scrubbed here and never trusted."""
        service_uid = self.env["zero_sudo.security.utils"]._get_service_uid(SERVICE_XMLID)
        if self.env.uid != service_uid:
            raise AccessError(self.env._("Only the exception reporter service account may report a daemon failure."))
        name = scrub.daemon_unit_name(unit)
        if not name:
            raise UserError(self.env._("The unit name is not a plain systemd unit name."))
        if kind not in scrub.DAEMON_KINDS:
            raise UserError(self.env._("Unknown failure kind."))
        result = result if result in scrub.DAEMON_RESULTS else "unknown"
        trace = "\n".join(
            (
                f"Daemon failure reported from outside Odoo: {name}",
                f"kind: {kind}",
                f"result: {result}",
                "Journal tail (scrubbed; exception messages are withheld):",
                scrub.scrub_daemon_tail(journal_tail),
            )
        )
        recorded = report_event(
            self.env.registry,
            "daemon",
            f"daemon.{kind}",
            name,
            f"result:{result}",
            f"systemd:{name}",
            trace,
            f"Daemon failure: {name} ({kind}, {result})",
        )
        return {"recorded": bool(recorded)}
