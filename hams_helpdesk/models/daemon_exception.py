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
from .exception_fingerprint import SERVICE_XMLID, _service_env, report_event


_UNIT_SUFFIXES = (".service", ".timer", ".socket", ".target")


# [@ANCHOR: hams_helpdesk:exc_unit_key_parts]
def unit_key_parts(name):
    """The fingerprint inputs shared by EVERY report about the systemd unit ``name``: the OnFailure
    template, a daemon's own consumer loop and any pager check that names the unit all land on one row
    and so on one ticket. They are the inputs a ``unit_failed`` / ``exit-code`` report always used, so a
    row and ticket opened before this existed keep counting. The report's own kind and result are in
    the sample trace, not in the key."""
    return ("daemon.unit_failed", name, "result:exit-code", f"systemd:{name}")


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
            f"Daemon failure: {name}",
            reporter=f"exception report ({kind})",
            key_parts=unit_key_parts(name),
        )
        return {"recorded": bool(recorded)}

    # [@ANCHOR: hams_helpdesk:exc_report_unit_failures]
    @api.model
    def _report_unit_failures(self, units, reporter, message=""):
        """Reports ``units`` (systemd unit names a pager check says are failing) through the same
        fingerprint, rate limit and own-cursor writer as ``report_daemon_failure``, so a check and the
        OnFailure unit land on one row and one ticket per unit. ``reporter`` is the check's name.

        Python only (the leading underscore keeps it off the RPC surface; the pager's own
        ``report_incident`` is the caller). Returns ``{unit: ticket id}`` for each plain unit named,
        the ticket id being False when no ticket exists (the new-ticket cap, or the write failed): the
        caller then keeps its own ticket, so a page is never lost to the dedup. Instances of a template
        unit are left out; they would all collapse into one bogus row."""
        found = {}
        for unit in units or ():
            text = str(unit or "").strip()
            if text and not text.endswith(_UNIT_SUFFIXES):
                # systemd's own default: `odoo` means `odoo.service`.
                text += ".service"
            name = scrub.daemon_unit_name(text)
            if not name or "@" in name or name in found:
                continue
            trace = "\n".join(
                (
                    f"Reported by the pager check: {reporter}",
                    scrub.scrub_daemon_tail(str(message).replace("; ", "\n")),
                )
            )
            report_event(
                self.env.registry,
                "daemon",
                "daemon.unit_failed",
                name,
                "result:unknown",
                f"systemd:{name}",
                trace,
                f"Daemon failure: {name}",
                reporter=reporter,
                key_parts=unit_key_parts(name),
            )
            found[name] = False
        if not found:
            return found
        # The writer committed on its own cursor; read what it wrote as the reporter service account.
        fingerprints = _service_env(self.env.cr)["hams_helpdesk.exception.fingerprint"]
        keys = {scrub.fingerprint(*unit_key_parts(name)): name for name in found}
        rows = fingerprints.search([("fingerprint", "in", list(keys))])
        for row in rows:
            found[keys[row.fingerprint]] = row.ticket_id.id or False
        return found
