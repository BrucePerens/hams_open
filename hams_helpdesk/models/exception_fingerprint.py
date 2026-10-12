# Copyright © Bruce Perens K6BP.
# SPDX-License-Identifier: AGPL-3.0-or-later
"""Automatic tickets for unhandled server exceptions.

One ``hams_helpdesk.exception.fingerprint`` row per distinct failure (exception type, innermost
addon frame, module, normalized route) and one ``server_exception`` ticket per row. A repeat adds to
the row's counter and, at most once a minute per fingerprint, refreshes the ticket.

Where it is called from: ir.http ``_handle_error`` (page controllers and every JSON-RPC / ORM call
from the web client) and ir.cron ``_callback`` (scheduled jobs, which include the mail queue). See
``ir_http_exception.py`` and ``ir_cron_exception.py``.

Safety properties, each tested:

* The write happens on its own cursor (``registry.cursor()``), so the rollback of the failed request
  does not lose it, and a failure of the writer cannot disturb the request.
* The writer never raises and never reports itself: one catch-all swallows and logs, and a
  thread-local flag stops a re-entrant report.
* A storm costs nothing: the per-process gate admits a fingerprint once per minute and the whole
  process at most GLOBAL_WRITES_PER_MINUTE; refused hits only bump an in-memory counter that the
  next admitted write adds to the row. Across workers, the row is locked and rechecked.
* Nothing but scrubbed text from ``exception_scrub`` is stored. The user is an internal user id.
* It runs as ``hams_helpdesk.user_exception_reporter_service``, which can touch only
  ``server_exception`` tickets and the fingerprint rows (no sudo()).
"""

import collections
import json
import logging
import threading
import time
from datetime import timedelta

from markupsafe import Markup
from odoo import api, fields, models

from . import exception_scrub as scrub

# Its own logger name: a failure to write a report is logged here and never reported again.
_reporter_logger = logging.getLogger("hams_helpdesk.exception_reporter")

SERVICE_XMLID = "hams_helpdesk.user_exception_reporter_service"
TICKET_TYPE = "server_exception"
PER_FINGERPRINT_SECONDS = 60
GLOBAL_WRITES_PER_MINUTE = 30
NEW_TICKETS_PER_HOUR = 50
SAMPLE_LIMIT = 5
SAMPLE_TRACE_LIMIT = 6000
GATE_MAX_ENTRIES = 5000
CLOSED_STAGES = ("resolved", "closed")



class _ReentryState(threading.local):
    """Per-thread flag: a report is being written on this thread."""

    active = False


_STATE = _ReentryState()
_GATE_LOCK = threading.Lock()
# fingerprint -> [monotonic time of the last admitted write, hits refused since the last write]
_GATE = {}
_RECENT_WRITES = collections.deque()


# [@ANCHOR: hams_helpdesk:exc_admit]
def _admit(fingerprint):
    """The per-process gate. Returns the number of earlier hits to fold into this write when this one
    may write, or None when it is refused (the hit is counted and added to the next write)."""
    now = time.monotonic()
    with _GATE_LOCK:
        entry = _GATE.setdefault(fingerprint, [None, 0])
        last = entry[0]
        if last is not None and now - last < PER_FINGERPRINT_SECONDS:
            entry[1] += 1
            return None
        while _RECENT_WRITES and now - _RECENT_WRITES[0] >= 60:
            _RECENT_WRITES.popleft()
        if len(_RECENT_WRITES) >= GLOBAL_WRITES_PER_MINUTE:
            entry[1] += 1
            return None
        _RECENT_WRITES.append(now)
        entry[0] = now
        if len(_GATE) > GATE_MAX_ENTRIES:
            stale = [key for key, value in _GATE.items() if value[0] is None or now - value[0] >= 60]
            for key in stale:
                if key != fingerprint and not _GATE[key][1]:
                    del _GATE[key]
        return entry[1]


# [@ANCHOR: hams_helpdesk:exc_settle]
def _settle(fingerprint, folded):
    """Takes ``folded`` hits off the pending counter once the write that carried them succeeded."""
    with _GATE_LOCK:
        entry = _GATE.get(fingerprint)
        if entry:
            entry[1] = max(0, entry[1] - folded)


# [@ANCHOR: hams_helpdesk:exc_service_env]
def _service_env(cr):
    """The reporter service account's environment on ``cr``. The account's id is resolved from its
    external id with a plain query (there is no request user on this cursor), then
    ``_get_service_env`` re-verifies that it really is a service account."""
    module, name = SERVICE_XMLID.split(".", 1)
    cr.execute(
        "SELECT res_id FROM ir_model_data WHERE module = %s AND name = %s AND model = 'res.users'",
        (module, name),
    )
    row = cr.fetchone()
    if not row:
        raise ValueError(f"{SERVICE_XMLID} is missing")
    env = api.Environment(cr, row[0], {})
    return env["zero_sudo.security.utils"]._get_service_env(SERVICE_XMLID)


# [@ANCHOR: hams_helpdesk:exc_write_report]
def _write_report(registry, data, folded):
    """Writes one occurrence on its own cursor. No try/except here on purpose: the service-account
    lookup must fail fast, and ``report_exception`` is the single place that swallows."""
    with registry.cursor() as cr:
        # The failed request's transaction may still be open, holding row locks, while this runs. A
        # wait on one of them would hang this worker (one thread waiting on itself, which Postgres
        # cannot see as a deadlock), so give up quickly: a timeout is swallowed like any failure.
        cr.execute("SET LOCAL lock_timeout = '3s'")  # audit-ignore-sql: constant; Tested by [@ANCHOR: hams_helpdesk:exc_write_report]
        cr.execute("SET LOCAL statement_timeout = '15s'")  # audit-ignore-sql: constant; Tested by [@ANCHOR: hams_helpdesk:exc_write_report]
        env = _service_env(cr)
        return env["hams_helpdesk.exception.fingerprint"]._record_occurrence(data, folded)


# [@ANCHOR: hams_helpdesk:exc_report_exception]
def report_exception(registry, exc, source, uid=None, logged_in=False, path="", rule=None, user_agent=""):
    """Turns ``exc`` into a ticket if it deserves one. Never raises, never recurses.

    ``source``: "http", "rpc" or "cron". ``uid`` is only ever stored as an integer. ``path`` is a URL
    path (any query string is dropped); ``rule`` is werkzeug's pattern for the matched route."""
    if _STATE.active:
        return False
    _STATE.active = True
    try:
        if scrub.is_expected(exc, source, logged_in, path, user_agent):
            return False
        route = scrub.normalize_route(path, rule)
        type_name, module, frame = scrub.describe(exc)
        key = scrub.fingerprint(type_name, module, frame, route)
        folded = _admit(key)
        if folded is None:
            return False
        data = {
            "name": f"{type_name.rsplit('.', 1)[-1]} in {module} at {route}"[:290],
            "fingerprint": key,
            "exc_type": type_name,
            "module": module,
            "frame": frame,
            "route": route,
            "source": source,
            "uid": uid if isinstance(uid, int) else None,
            "trace": scrub.scrubbed_trace(exc)[:SAMPLE_TRACE_LIMIT],
        }
        written = _write_report(registry, data, folded)
        if written:
            _settle(key, folded)
        return bool(written)
    except Exception as error:  # audit-ignore-catch-all: Tested by [@ANCHOR: hams_helpdesk:exc_report_exception]
        # A failure here must never reach the request or the cron job, and must not be reported
        # again: log it on this module's own logger and stop.
        _reporter_logger.error("Could not record a server exception ticket: %r", error)
        return False
    finally:
        _STATE.active = False


class ExceptionFingerprint(models.Model):
    _name = "hams_helpdesk.exception.fingerprint"
    _description = "Server exception fingerprint"
    _order = "last_seen desc, id desc"

    name = fields.Char(string="Failure", required=True, readonly=True)
    fingerprint = fields.Char(required=True, index=True, readonly=True)
    exc_type = fields.Char(string="Exception", readonly=True)
    module = fields.Char(readonly=True, index=True)
    frame = fields.Char(string="Innermost frame", readonly=True)
    route = fields.Char(string="Route", readonly=True, index=True)
    source = fields.Selection(
        [("http", "Page"), ("rpc", "RPC / ORM call"), ("cron", "Scheduled job")],
        readonly=True,
    )
    occurrence_count = fields.Integer(string="Occurrences", readonly=True)
    first_seen = fields.Datetime(readonly=True)
    last_seen = fields.Datetime(readonly=True, index=True)
    last_written_at = fields.Datetime(readonly=True)
    last_user_id = fields.Integer(
        string="Last user id", readonly=True,
        help="Internal user id of the last logged-in user affected. Never a name or an email.",
    )
    release_marker = fields.Char(
        readonly=True,
        help="Newest module-install timestamp when this last happened. When a closed ticket "
        "recurs under a different marker, it is reopened.",
    )
    samples_json = fields.Text(string="Last sample traces", readonly=True)
    ticket_id = fields.Many2one("hams_helpdesk.ticket", string="Ticket", ondelete="set null", index=True, readonly=True)
    ticket_stage = fields.Selection(related="ticket_id.stage", string="Ticket stage", store=True)

    _fingerprint_unique = models.Constraint(
        "UNIQUE(fingerprint)", "One row per exception fingerprint."
    )

    # [@ANCHOR: hams_helpdesk:exc_release_marker]
    @api.model
    def _release_marker(self):
        """Changes whenever any module is installed or upgraded, which is what a deploy does."""
        self.env.cr.execute(
            "SELECT COALESCE(to_char(max(write_date), 'YYYYMMDDHH24MISS'), '') "
            "FROM ir_module_module WHERE state = 'installed'"
        )
        return self.env.cr.fetchone()[0]

    # [@ANCHOR: hams_helpdesk:exc_ticket_body]
    def _ticket_body(self):
        """The ticket description: counters and the newest sample, escaped."""
        self.ensure_one()
        samples = json.loads(self.samples_json or "[]")
        latest = samples[-1] if samples else {}
        rows = (
            ("Occurrences", self.occurrence_count),
            ("First seen (UTC)", self.first_seen),
            ("Last seen (UTC)", self.last_seen),
            ("Exception", self.exc_type),
            ("Module", self.module),
            ("Innermost frame", self.frame),
            ("Route", self.route),
            ("Source", self.source),
            ("Fingerprint", self.fingerprint),
        )
        # Markup.format escapes every argument, so nothing here is interpolated unescaped.
        item = Markup("<li><b>{}:</b> {}</li>")
        lines = Markup("").join(item.format(label, str(value)) for label, value in rows)
        intro = Markup(
            "<p>Created automatically from an unhandled server exception. Personal data, query strings, "
            "request bodies and session data are never recorded. The newest sample is below; "
            "the last samples are on the Server Exceptions list.</p>"
        )
        return intro + Markup("<ul>{}</ul><pre>{}</pre>").format(lines, latest.get("trace", ""))

    # [@ANCHOR: hams_helpdesk:exc_record_occurrence]
    @api.model
    def _record_occurrence(self, data, folded):
        """Records one admitted occurrence. Returns True when it wrote.

        Rechecks, under a row lock, that no other worker wrote this fingerprint in the last
        PER_FINGERPRINT_SECONDS. Creates the ticket (unless NEW_TICKETS_PER_HOUR was reached, in which
        case the row still counts and a later write creates it), refreshes it, and reopens a closed one
        only when a release happened since the last occurrence."""
        now = fields.Datetime.now()
        marker = self._release_marker()
        sample = {
            "at": fields.Datetime.to_string(now),
            "uid": data["uid"],
            "source": data["source"],
            "route": data["route"],
            "trace": data["trace"],
        }
        record = self.search([("fingerprint", "=", data["fingerprint"])], limit=1)
        reopen = False
        if record:
            if not record.try_lock_for_update():
                return False
            last = record.last_written_at
            if last and (now - last).total_seconds() < PER_FINGERPRINT_SECONDS:
                return False
            samples = (json.loads(record.samples_json or "[]") + [sample])[-SAMPLE_LIMIT:]
            ticket_closed = record.ticket_id.stage in CLOSED_STAGES
            reopen = bool(ticket_closed and record.release_marker != marker)
            record.write(
                {
                    "occurrence_count": record.occurrence_count + 1 + folded,
                    "last_seen": now,
                    "last_written_at": now,
                    "last_user_id": data["uid"] or record.last_user_id,
                    "release_marker": marker,
                    "samples_json": json.dumps(samples),
                }
            )
        else:
            record = self.create(
                {
                    "name": data["name"],
                    "fingerprint": data["fingerprint"],
                    "exc_type": data["exc_type"],
                    "module": data["module"],
                    "frame": data["frame"],
                    "route": data["route"],
                    "source": data["source"],
                    "occurrence_count": 1 + folded,
                    "first_seen": now,
                    "last_seen": now,
                    "last_written_at": now,
                    "last_user_id": data["uid"] or 0,
                    "release_marker": marker,
                    "samples_json": json.dumps([sample]),
                }
            )
        self._sync_ticket(record, now, reopen)
        return True

    # [@ANCHOR: hams_helpdesk:exc_sync_ticket]
    @api.model
    def _sync_ticket(self, record, now, reopen):
        """Creates or refreshes the ticket for ``record``; reopens a closed one when ``reopen``."""
        Ticket = self.env["hams_helpdesk.ticket"]
        body = record._ticket_body()
        if record.ticket_id:
            vals = {"description": body}
            if reopen:
                vals["stage"] = "new"
            record.ticket_id.write(vals)
            if reopen:
                record.ticket_id.message_post(
                    body=self.env._("Recurred after a release; reopened automatically."),
                    subtype_xmlid="mail.mt_note",
                )
            return
        since = now - timedelta(hours=1)
        recent = Ticket.search_count([("ticket_type", "=", TICKET_TYPE), ("create_date", ">=", since)])
        if recent >= NEW_TICKETS_PER_HOUR:
            _reporter_logger.warning(
                "Not opening another server exception ticket: %s already opened in the last hour",
                recent,
            )
            return
        name = "Server exception: " + record.name
        ticket = Ticket.create(
            {
                "name": name[:290],
                "description": body,
                "ticket_type": TICKET_TYPE,
                "priority": "1",
                "company_id": self.env.company.id,
            }
        )
        record.ticket_id = ticket.id
