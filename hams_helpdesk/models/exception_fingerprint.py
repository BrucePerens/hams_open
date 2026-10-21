# Copyright © Bruce Perens K6BP.
# SPDX-License-Identifier: AGPL-3.0-or-later
"""Automatic tickets for unhandled server exceptions.

One ``hams_helpdesk.exception.fingerprint`` row per distinct failure (exception type, innermost
addon frame, module, normalized route) and one ``server_exception`` ticket per row. A repeat adds to
the row's counter and, at most once a minute per fingerprint, refreshes the ticket.

Where it is called from: ir.http ``_handle_error`` (page controllers and every JSON-RPC / ORM call
from the web client) and ir.cron ``_callback`` (scheduled jobs, which include the mail queue), mail.mail
``_postprocess_sent_message`` (a message that fails to send, which Odoo records on the mail and does
not raise) and mail.thread ``_routing_handle_bounce``. See ``ir_http_exception.py``,
``ir_cron_exception.py`` and ``mail_exception.py``.

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
import contextlib
import json
import logging
import threading
import time
from datetime import timedelta

from markupsafe import Markup
from odoo import api, fields, http, models

from . import exception_scrub as scrub

# Its own logger name: a failure to write a report is logged here and never reported again.
_reporter_logger = logging.getLogger("hams_helpdesk.exception_reporter")

SERVICE_XMLID = "hams_helpdesk.user_exception_reporter_service"
TICKET_TYPE = "server_exception"
PER_FINGERPRINT_SECONDS = 60
GLOBAL_WRITES_PER_MINUTE = 30
NEW_TICKETS_PER_HOUR = 50
# A new ticket pages the on-duty admin (an email and a toast) only when none did in this long; the
# others in a burst are still created and assigned, just silent, and the next one that does page says
# how many were silent. A release that breaks twenty pages is one page, not twenty.
NOTIFY_WINDOW = timedelta(minutes=30)
SAMPLE_LIMIT = 5
SAMPLE_TRACE_LIMIT = 6000
GATE_MAX_ENTRIES = 5000
CLOSED_STAGES = ("resolved", "closed")
# A closed ticket whose failure was last seen at least this long ago is reopened by the next occurrence
# even with no release in between: a failure that comes back after a quiet day is not the one that was closed.
QUIET_REOPEN = timedelta(hours=24)


class _ReentryState(threading.local):
    """Per-thread flag: a report is being written on this thread."""

    active = False


_STATE = _ReentryState()
_GATE_LOCK = threading.Lock()
# fingerprint -> [monotonic time of the last admitted write, hits refused since the last write,
# reporters this process has already recorded for it]
_GATE = {}
# A reporter label is only ever a check name or a fixed word; the set per fingerprint stays small.
GATE_MAX_REPORTERS = 50
_RECENT_WRITES = collections.deque()


# [@ANCHOR: hams_helpdesk:exc_admit]
def _admit(fingerprint, reporter=None):
    """The per-process gate. Returns the number of earlier hits to fold into this write when this one
    may write, or None when it is refused (the hit is counted and added to the next write).

    A ``reporter`` this process has not yet recorded for the fingerprint is admitted inside the
    one-minute window (the process-wide cap still applies): a second check reporting the same failure
    must show up on the ticket at once, and it only happens once per reporter."""
    now = time.monotonic()
    with _GATE_LOCK:
        entry = _GATE.setdefault(fingerprint, [None, 0, set()])
        last = entry[0]
        new_reporter = bool(reporter) and reporter not in entry[2]
        if last is not None and now - last < PER_FINGERPRINT_SECONDS and not new_reporter:
            entry[1] += 1
            return None
        while _RECENT_WRITES and now - _RECENT_WRITES[0] >= 60:
            _RECENT_WRITES.popleft()
        if len(_RECENT_WRITES) >= GLOBAL_WRITES_PER_MINUTE:
            entry[1] += 1
            return None
        _RECENT_WRITES.append(now)
        entry[0] = now
        if reporter and len(entry[2]) < GATE_MAX_REPORTERS:
            entry[2].add(reporter)
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


# [@ANCHOR: hams_helpdesk:exc_without_request]
@contextlib.contextmanager
def _without_request():
    """Hides the HTTP request this thread is serving, if any, while the report is written.

    The writer runs on its own cursor as the service account. Code under ticket creation (website
    rules, mail, bus) looks at the ambient ``request`` when there is one, and for a failed request
    that request can be half-built (a session with no valid user): the writer would then depend on
    the very request whose failure it is reporting. A cron job has no request, so this is the path the
    writer is written for."""
    if not http.request:
        yield
        return
    with http.borrow_request():
        yield


# [@ANCHOR: hams_helpdesk:exc_write_report]
def _write_report(registry, data, folded):
    """Writes one occurrence on its own cursor. No try/except here on purpose: the service-account
    lookup must fail fast, and ``report_exception`` is the single place that swallows."""
    with _without_request(), registry.cursor() as cr:
        # The failed request's transaction may still be open, holding row locks, while this runs. A
        # wait on one of them would hang this worker (one thread waiting on itself, which Postgres
        # cannot see as a deadlock), so give up quickly: a timeout is swallowed like any failure.
        cr.execute("SET LOCAL lock_timeout = '3s'")  # audit-ignore-sql: constant; Tested by [@ANCHOR: hams_helpdesk:exc_write_report]
        cr.execute("SET LOCAL statement_timeout = '15s'")  # audit-ignore-sql: constant; Tested by [@ANCHOR: hams_helpdesk:exc_write_report]
        env = _service_env(cr)
        return env["hams_helpdesk.exception.fingerprint"]._record_occurrence(data, folded)


# [@ANCHOR: hams_helpdesk:exc_report_event]
def report_event(
    registry, source, type_name, module, frame, route, trace, name, uid=None, reporter=None, key_parts=None
):
    """Turns an already-described failure into a ticket. Never raises, never recurses.

    The shared tail of ``report_exception`` and of the reporters that have no Python exception to
    describe (a mail that failed to send, an external daemon). The caller supplies text that is
    already safe to store: a fixed vocabulary or counts, never a message, address or body. ``trace``
    and ``name`` are cut to the storage limits here.

    ``reporter`` says who reported it (a pager check's name, or a fixed word): the ticket lists every
    reporter, and a new one is recorded at once. ``key_parts`` is a ``(type, module, frame, route)``
    tuple that sets the fingerprint when it should be shared by reports whose own text differs (every
    report about one systemd unit, see ``daemon_exception.unit_key_parts``); the row still stores
    the values of the report that created it."""
    if _STATE.active:
        return False
    _STATE.active = True
    try:
        key = scrub.fingerprint(*(key_parts or (type_name, module, frame, route)))
        reporter = str(reporter)[:80] if reporter else None
        folded = _admit(key, reporter)
        if folded is None:
            return False
        data = {
            "name": str(name)[:290],
            "fingerprint": key,
            "exc_type": type_name,
            "module": module,
            "frame": frame,
            "route": route,
            "source": source,
            "uid": uid if isinstance(uid, int) else None,
            "trace": str(trace)[:SAMPLE_TRACE_LIMIT],
            "reporter": reporter,
        }
        written = _write_report(registry, data, folded)
        if written:
            _settle(key, folded)
        return bool(written)
    except Exception as error:  # audit-ignore-catch-all: Tested by [@ANCHOR: hams_helpdesk:exc_report_exception]
        # A failure here must never reach the request, the cron job or the mail queue, and must not
        # be reported again: log it on this module's own logger and stop.
        _reporter_logger.error("Could not record a server exception ticket: %r", error, exc_info=True)
        return False
    finally:
        _STATE.active = False


# [@ANCHOR: hams_helpdesk:exc_report_exception]
def report_exception(registry, exc, source, uid=None, logged_in=False, path="", rule=None, user_agent=""):
    """Turns ``exc`` into a ticket if it deserves one. Never raises, never recurses.

    ``source``: "http", "rpc" or "cron". ``uid`` is only ever stored as an integer. ``path`` is a URL
    path (any query string is dropped); ``rule`` is werkzeug's pattern for the matched route."""
    if _STATE.active:
        return False
    try:
        if scrub.is_expected(exc, source, logged_in, path, user_agent):
            return False
        route = scrub.normalize_route(path, rule)
        type_name, module, frame = scrub.describe(exc)
        trace = scrub.scrubbed_trace(exc)
    except Exception as error:  # audit-ignore-catch-all: Tested by [@ANCHOR: hams_helpdesk:exc_report_exception]
        _reporter_logger.error("Could not describe a server exception: %r", error)
        return False
    return report_event(
        registry, source, type_name, module, frame, route, trace,
        f"{type_name.rsplit('.', 1)[-1]} in {module} at {route}", uid=uid,
    )


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
        [
            ("http", "Page"),
            ("rpc", "RPC / ORM call"),
            ("cron", "Scheduled job"),
            ("mail", "Outgoing / incoming mail"),
            ("daemon", "External daemon"),
        ],
        readonly=True,
    )
    occurrence_count = fields.Integer(string="Occurrences", readonly=True)
    first_seen = fields.Datetime(readonly=True)
    last_seen = fields.Datetime(readonly=True, index=True)
    last_written_at = fields.Datetime(readonly=True)
    notified_at = fields.Datetime(
        readonly=True,
        help="When opening this row's ticket paged the on-duty admin. Empty when it was opened silently.",
    )
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
    reporters_json = fields.Text(
        string="Reported by", readonly=True,
        help="JSON list of everything that reported this failure: pager check names, the exception "
        "reporter, the daemon itself. Never message text.",
    )
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

    # [@ANCHOR: hams_helpdesk:exc_reporters]
    def _reporters(self):
        """Everything that has reported this failure, oldest first. Empty for a row from before
        reporters were recorded."""
        self.ensure_one()
        return json.loads(self.reporters_json or "[]")

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
            ("Reported by", ", ".join(self._reporters())),
            ("Fingerprint", self.fingerprint),
        )
        rows = tuple(row for row in rows if row[1] or row[0] != "Reported by")
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
        reporter = data.get("reporter")
        if record:
            if not record.try_lock_for_update():
                return False
            last = record.last_written_at
            reporters = record._reporters()
            # A reporter the row has not listed yet is recorded even inside the window: two checks
            # that see the same failure at the same moment are both named on the one ticket.
            new_reporter = bool(reporter) and reporter not in reporters and len(reporters) < GATE_MAX_REPORTERS
            if last and (now - last).total_seconds() < PER_FINGERPRINT_SECONDS and not new_reporter:
                return False
            if new_reporter:
                reporters.append(reporter)
            samples = (json.loads(record.samples_json or "[]") + [sample])[-SAMPLE_LIMIT:]
            ticket_closed = record.ticket_id.stage in CLOSED_STAGES
            reopen = ticket_closed and self._reopen_reason(record, marker, now)
            record.write(
                {
                    "occurrence_count": record.occurrence_count + 1 + folded,
                    "last_seen": now,
                    "last_written_at": now,
                    "last_user_id": data["uid"] or record.last_user_id,
                    "release_marker": marker,
                    "samples_json": json.dumps(samples),
                    "reporters_json": json.dumps(reporters),
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
                    "reporters_json": json.dumps([reporter] if reporter else []),
                }
            )
        self._sync_ticket(record, now, reopen)
        return True

    # [@ANCHOR: hams_helpdesk:exc_reopen_reason]
    @api.model
    def _reopen_reason(self, record, marker, now):
        """Why a recurrence of ``record`` reopens its closed ticket, or "" when it stays closed (counted
        only). Three rules: a release since the last occurrence; a daemon / systemd unit failing again
        (always: a unit that fails after its ticket was closed is still broken); and a quiet period of
        QUIET_REOPEN since the failure was last seen."""
        if record.source == "daemon":
            return "a unit failed again after its ticket was closed"
        if record.release_marker != marker:
            return "it recurred after a release"
        if record.last_seen and now - record.last_seen >= QUIET_REOPEN:
            return "it recurred after a quiet period of 24 hours or more"
        return ""

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
                self._announce_reopen(record, reopen, now)
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
        quiet, silent_since_page = self._notification_state(now)
        # mail_auto_subscribe_no_notify: Odoo's own "You have been assigned" email would repeat the
        # helpdesk's "Ticket Assigned" one and ignore the window.
        ticket = Ticket.with_context(
            mail_auto_subscribe_no_notify=True,
            hams_exception_quiet=quiet,
            hams_exception_silent_count=silent_since_page,
        ).create(
            {
                "name": name[:290],
                "description": body,
                "ticket_type": TICKET_TYPE,
                "priority": "1",
                "company_id": self.env.company.id,
            }
        )
        record.ticket_id = ticket.id
        if not quiet:
            record.notified_at = now

    # [@ANCHOR: hams_helpdesk:exc_announce_reopen]
    @api.model
    def _announce_reopen(self, record, reason, now):
        """The note on a reopened ticket. It pages the assignee as a new ticket would, under the same
        NOTIFY_WINDOW: inside the window of an earlier page it is a plain note."""
        ticket = record.ticket_id
        quiet, _silent = self._notification_state(now)
        assignee = ticket.user_id.partner_id
        page = bool(assignee) and not quiet
        ticket.message_post(
            body=self.env._(
                "Recurred (%(count)s occurrences in all; %(reason)s); reopened automatically.",
                count=record.occurrence_count,
                reason=reason,
            ),
            subtype_xmlid="mail.mt_note",
            partner_ids=assignee.ids if page else [],
        )
        if page:
            record.notified_at = now

    # [@ANCHOR: hams_helpdesk:exc_notification_state]
    @api.model
    def _notification_state(self, now):
        """(quiet, silent): whether a ticket opened now stays silent because another one paged the
        on-duty admin within NOTIFY_WINDOW, and, when it is not quiet, how many tickets were opened
        silently since the last page."""
        last_page = self.search([("notified_at", "!=", False)], order="notified_at desc", limit=1)
        if last_page and now - last_page.notified_at < NOTIFY_WINDOW:
            return True, 0
        domain = [("ticket_type", "=", TICKET_TYPE)]
        if last_page:
            domain.append(("id", ">", last_page.ticket_id.id or 0))
        return False, self.env["hams_helpdesk.ticket"].search_count(domain)
