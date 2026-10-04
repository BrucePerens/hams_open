# Copyright © Bruce Perens K6BP.
# SPDX-License-Identifier: AGPL-3.0-or-later
"""The Odoo half of the event-driven ticket-triage trigger (Bruce, NIGHT_PLAN decision 222: "Can we get
a notification of an incoming ticket from Odoo and run it immediately?").

After a ticket is created and the transaction has committed, drop a tiny spool file
`ticket-<id>.json` into the triage spool directory. The `ticket.triage.path` systemd unit
(hams_shared/tools/infrastructure.py) watches that directory and starts the triage daemon
(hams_com daemons/ticket_triage_agent, `main.py --event`). The mechanism is the repo's existing
"spool directory plus .path unit" shape (backup_management's pgbackrest request spool is the first
user), chosen over a RabbitMQ queue because the daemon stays a one-shot (no always-on consumer, no
broker credential in the triage service), the filesystem is up whenever Odoo is, and a file simply
stays on disk until the daemon, or the fallback timer, gets to it: a notification cannot be lost by
the daemon being down.

Safety properties, each held by code below:
  * fire-and-forget: runs from cr.postcommit, so it adds nothing to the create's transaction, and
    EVERY failure is swallowed and logged: a full disk, a missing directory, a bad permission, never
    reaches the ticket create (and a post-commit hook cannot undo a committed ticket anyway);
  * the trigger carries only the integer ticket id, in the file name and as the sole JSON field. No
    subject, body, address or any other text a ticket author controls ever leaves the database, and the
    daemon never reads the content at all;
  * a bounded spool: at most MAX_SPOOL_FILES wake-ups wait at once. An unauthenticated sender flooding
    inbound mail can fill that and no more; the surplus is dropped (the daemon's own pre-check finds the
    waiting tickets in Odoo on its next pass, and the fallback timer exists for exactly this), and the
    daemon's daily and per-run caps bound the model work no matter how many files exist;
  * no sudo() and no ORM access at all: it takes ids and writes a file. The spool directory is
    odoo-owned 0700; where it does not exist (a dev box, a test host) the function does nothing, it
    never creates directories.
"""
import json
import logging
import os
import re

_logger = logging.getLogger(__name__)

SPOOL_DIR_ENV = "HAMS_TRIAGE_SPOOL_DIR"
DEFAULT_SPOOL_DIR = "/opt/hams/spool/ticket_triage"
MAX_SPOOL_FILES = 500
# Must match the daemon's own _SPOOL_NAME_RE and the path unit's PathExistsGlob.
_WAKEUP_NAME_RE = re.compile(r"^ticket-\d+\.json$")


def spool_dir():
    return os.environ.get(SPOOL_DIR_ENV) or DEFAULT_SPOOL_DIR


# [@ANCHOR: hams_helpdesk:triage_wakeup_write]
def write_triage_wakeups(ticket_ids):
    """Writes one wake-up file per id. Never raises; returns how many files it wrote."""
    written = 0
    try:
        directory = spool_dir()
        if not os.path.isdir(directory):
            _logger.debug("Triage spool %s does not exist: no wake-up written (not provisioned here).", directory)
            return 0
        waiting = sum(1 for name in os.listdir(directory) if _WAKEUP_NAME_RE.match(name))
        for ticket_id in ticket_ids:
            if waiting >= MAX_SPOOL_FILES:
                _logger.warning(
                    "Triage spool already holds %s wake-ups: not adding more (the fallback timer will "
                    "find the remaining new tickets).", waiting,
                )
                break
            ticket_id = int(ticket_id)
            final = os.path.join(directory, f"ticket-{ticket_id}.json")
            if os.path.lexists(final):
                continue  # already announced; the same ticket never counts twice
            # A dot-name temporary file does not match the path unit's glob, so the daemon never
            # sees a half-written file; os.replace publishes it atomically.
            temporary = os.path.join(directory, f".ticket-{ticket_id}.json.tmp")
            try:
                with open(temporary, "w") as handle:
                    json.dump({"ticket_id": ticket_id}, handle)
                os.replace(temporary, final)
            except OSError:
                try:
                    os.unlink(temporary)
                except OSError as cleanup_error:
                    _logger.debug("No temporary wake-up file to remove (%s).", cleanup_error)
                raise
            waiting += 1
            written += 1
    except Exception:  # audit-ignore-catch-all: a notification failure must never reach the ticket create, which has already committed
        _logger.exception("Could not write the ticket-triage wake-up (the fallback timer will find the ticket).")
    return written
