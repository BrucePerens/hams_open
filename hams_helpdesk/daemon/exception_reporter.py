# Copyright © Bruce Perens K6BP.
# SPDX-License-Identifier: AGPL-3.0-or-later
"""Reports a failed daemon to Odoo's exception-ticket pipeline. Standard library only.

Two uses, both ending in ``hams_helpdesk.exception.fingerprint.report_daemon_failure`` over Odoo's
JSON-2 API, authenticated with the daemon key the Daemon Key Manager issues to the exception
reporter service account. The key file is ``EXCEPTION_REPORTER_KEY_FILE`` or, by default,
``/opt/hams/etc/keys/exception_reporter.env`` (odoo-owned, 0600). ``ODOO_KEY_FILE`` is deliberately NOT
used: a daemon's own unit sets it to the daemon's own key, and the endpoint refuses every account but
the reporter's. No secret lives in code or on a command line:

* ``python3 exception_reporter.py report-unit <unit>``: what the systemd template unit
  ``hams-exception-report@.service`` runs when a unit named ``OnFailure=`` fails. It sends the unit
  name, systemd's Result word and the last journal lines; Odoo scrubs the lines.
* ``reporting_failures("<name>")``: a context manager / decorator a Python daemon wraps its consumer
  loop in. An uncaught exception is reported (the class and the frames, never the message) and then
  raised again, so the daemon still stops and systemd still restarts it.

Reporting never hides the original failure and never raises into it: a failed report is written to
stderr as a status code and nothing more (no URL, key or response body).
"""

import contextlib
import json
import os
import subprocess
import sys
import time
import traceback
import urllib.error
import urllib.request

DEFAULT_URL = "http://odoo:8069"
DEFAULT_KEY_FILE = "/opt/hams/etc/keys/exception_reporter.env"
USER_AGENT = "hams.com-daemon-client/1.0"
REPORT_INTERVAL_SECONDS = 60
JOURNAL_LINES = 20
# report-unit (the OnFailure unit) waits this long for an Odoo that is down or restarting before it gives up. A unit that
# fails during a deploy or restart is exactly when Odoo is unreachable, and giving up at once leaves a failed
# hams-exception-report@ instance behind that the systemd failed-services check then pages for, with no ticket filed.
# The template unit's TimeoutStartSec must stay longer than this (hams_shared tools/test_infrastructure.py).
REPORT_ODOO_WAIT_SECONDS = 600
REPORT_BACKOFF_FIRST_SECONDS = 5
REPORT_BACKOFF_MAX_SECONDS = 60
UNAVAILABLE_HTTP_CODES = frozenset({502, 503, 504})
_LAST_REPORT = {}


class ReportError(Exception):
    """The report could not be delivered. The message never holds a key, URL or response body."""


class OdooUnavailable(ReportError):
    """Odoo did not answer (connection refused or timed out, or a proxy's 502/503/504): worth retrying."""


# [@ANCHOR: hams_helpdesk:daemon_load_key]
def load_key(key_file):
    """The API key from a daemon-key-manager ``.env`` file (``ODOO_RPC_KEY=...``)."""
    if not key_file or not os.path.isfile(key_file):
        raise ReportError("the exception reporter key file does not exist or cannot be read")
    values = {}
    with open(key_file, "r", encoding="utf-8") as handle:
        for line in handle:
            line = line.strip()
            if line and not line.startswith("#") and "=" in line:
                name, _, value = line.partition("=")
                values[name.strip()] = value.strip()
    key = values.get("ODOO_RPC_KEY")
    if not key:
        raise ReportError("the key file has no ODOO_RPC_KEY")
    return key


# [@ANCHOR: hams_helpdesk:daemon_post_report]
def post_report(payload, url=None, database=None, key_file=None, timeout=15):
    """Sends one report. Returns Odoo's answer (a dict). Raises ReportError on any failure."""
    base = (url or os.environ.get("ODOO_URL") or DEFAULT_URL).rstrip("/")
    key = load_key(key_file or os.environ.get("EXCEPTION_REPORTER_KEY_FILE") or DEFAULT_KEY_FILE)
    headers = {
        "Authorization": f"Bearer {key}",
        "Content-Type": "application/json",
        "User-Agent": USER_AGENT,
    }
    database = database or os.environ.get("ODOO_DB") or os.environ.get("DB_NAME")
    if database:
        headers["X-Odoo-Database"] = database
    request = urllib.request.Request(
        f"{base}/json/2/hams_helpdesk.exception.fingerprint/report_daemon_failure",
        data=json.dumps(payload).encode("utf-8"),
        headers=headers,
        method="POST",
    )
    try:
        with urllib.request.urlopen(request, timeout=timeout) as response:
            return json.loads(response.read().decode("utf-8"))
    except urllib.error.HTTPError as error:
        if error.code in UNAVAILABLE_HTTP_CODES:
            raise OdooUnavailable(f"Odoo answered HTTP {error.code}") from None
        raise ReportError(f"Odoo answered HTTP {error.code}") from None
    except (urllib.error.URLError, OSError) as error:
        raise OdooUnavailable(f"could not reach Odoo ({type(error).__name__})") from None
    except ValueError as error:
        raise ReportError(f"could not reach Odoo ({type(error).__name__})") from None


# [@ANCHOR: hams_helpdesk:daemon_journal_tail]
def journal_tail(unit, lines=JOURNAL_LINES):
    """The last journal lines of ``unit``, as written, or "" when the journal cannot be read."""
    try:
        done = subprocess.run(
            ["journalctl", "--unit", unit, "--lines", str(lines), "--no-pager", "--output", "cat"],
            capture_output=True, text=True, timeout=20, check=False,
        )
    except (OSError, subprocess.SubprocessError):
        return ""
    return done.stdout if done.returncode == 0 else ""


# [@ANCHOR: hams_helpdesk:daemon_unit_result]
def unit_result(unit):
    """systemd's Result for ``unit`` ("exit-code", "signal", ...), or "" when it cannot be read."""
    try:
        done = subprocess.run(
            ["systemctl", "show", "--property=Result", "--value", unit],
            capture_output=True, text=True, timeout=20, check=False,
        )
    except (OSError, subprocess.SubprocessError):
        return ""
    return done.stdout.strip() if done.returncode == 0 else ""


# [@ANCHOR: hams_helpdesk:daemon_report_unit]
def report_unit(unit, wait_seconds=None, sleep=time.sleep, monotonic=time.monotonic, **options):
    """What the OnFailure unit does: report ``unit`` with its result and journal tail. While Odoo is unavailable (a restart
    or deploy) it retries with a growing delay for up to ``wait_seconds`` (REPORT_ODOO_WAIT_SECONDS), then raises the last
    error. Any other failure (a rejected key, a 4xx) is raised at once. The payload is read once, before the first try."""
    payload = {
        "unit": unit,
        "kind": "unit_failed",
        "result": unit_result(unit),
        "journal_tail": journal_tail(unit),
    }
    budget = REPORT_ODOO_WAIT_SECONDS if wait_seconds is None else wait_seconds
    started = monotonic()
    delay = REPORT_BACKOFF_FIRST_SECONDS
    while True:
        try:
            return post_report(payload, **options)
        except OdooUnavailable as error:
            if monotonic() - started + delay > budget:
                raise
            print(f"Odoo is not answering ({error}); retrying in {delay}s", file=sys.stderr)
            sleep(delay)  # audit-ignore-sleep: bounded wait for Odoo to come back
            delay = min(delay * 2, REPORT_BACKOFF_MAX_SECONDS)


# [@ANCHOR: hams_helpdesk:daemon_frames_only]
def frames_only(exc):
    """The exception as text with no message: the class and each frame's module, line and function.
    The message and locals of an exception routinely carry the data being processed."""
    lines = [f"{type(exc).__module__}.{type(exc).__qualname__}: <message withheld>"]
    for frame, lineno in traceback.walk_tb(exc.__traceback__):
        name = frame.f_globals.get("__name__", "?")
        lines.append(f"  {name}:{lineno} in {frame.f_code.co_name}")
    return "\n".join(lines)


# [@ANCHOR: hams_helpdesk:daemon_report_exception]
def report_exception(daemon_name, exc, **options):
    """Reports an uncaught exception of the daemon ``daemon_name`` (its unit name, for example
    "ses-inbound-mail-ingest.service"). At most once a minute per name in this process; returns the
    answer, or None when skipped or when the report could not be delivered. Never raises."""
    now = time.monotonic()
    last = _LAST_REPORT.get(daemon_name)
    if last is not None and now - last < REPORT_INTERVAL_SECONDS:
        return None
    _LAST_REPORT[daemon_name] = now
    try:
        return post_report(
            {
                "unit": daemon_name,
                "kind": "consumer_exception",
                "result": "exit-code",
                "journal_tail": frames_only(exc),
            },
            **options,
        )
    except ReportError as error:
        print(f"exception report not delivered: {error}", file=sys.stderr)
        return None


# [@ANCHOR: hams_helpdesk:daemon_reporting_failures]
class reporting_failures(contextlib.ContextDecorator):
    """Context manager and decorator: report an uncaught exception, then let it propagate unchanged.

        with reporting_failures("ses-inbound-mail-ingest.service"):
            consume_forever()

    Only ``Exception`` is reported: a stop signal (SystemExit, KeyboardInterrupt) is a normal stop."""

    def __init__(self, daemon_name, **options):
        self.daemon_name = daemon_name
        self.options = options

    def __enter__(self):
        return self

    def __exit__(self, exc_type, exc, traceback_):
        if exc is not None and isinstance(exc, Exception):
            report_exception(self.daemon_name, exc, **self.options)
        return False


def main(argv=None):
    argv = list(sys.argv[1:] if argv is None else argv)
    if len(argv) != 2 or argv[0] != "report-unit":
        print("usage: exception_reporter.py report-unit <unit>", file=sys.stderr)
        return 2
    try:
        answer = report_unit(argv[1])
    except ReportError as error:
        print(f"exception report not delivered: {error}", file=sys.stderr)
        return 1
    print(f"exception report for {argv[1]}: {json.dumps(answer)}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
