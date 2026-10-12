# Copyright © Bruce Perens K6BP.
# SPDX-License-Identifier: AGPL-3.0-or-later
"""Pure helpers for the automatic server-exception tickets: which exceptions are worth a ticket,
the fingerprint that groups repeats, and the scrubbed trace that goes into the ticket.

Nothing here touches the database or the request, so it is safe to call while a transaction is
aborted. What may reach a ticket:

* the exception class, and for a short allow-list of classes whose message is built from code
  rather than from data (AttributeError, TypeError, ...) the message, scrubbed;
* for every frame, the module, line number, function and the source line (scrubbed);
* nothing from locals, request bodies, form values, cookies, headers, query strings or session ids.
"""

import hashlib
import linecache
import re
import traceback

import psycopg2
import werkzeug.exceptions
from odoo import exceptions
from odoo.addons.base.models.ir_qweb import QWebError

# Messages of these classes come from code, not from data, so they are kept (scrubbed). Every other
# class has its message withheld: a KeyError, ValueError or database error routinely carries the
# value that was being processed (a name, an email, a callsign, a row).
MESSAGE_SAFE_TYPES = (
    AttributeError,
    TypeError,
    NameError,
    ZeroDivisionError,
    NotImplementedError,
    AssertionError,
    RecursionError,
)
MESSAGE_LIMIT = 200
CODE_LINE_LIMIT = 160
MAX_FRAMES = 25
MAX_CHAIN = 4
# Request paths that only scanners ask for. An exception on one of these, from an anonymous caller,
# is noise.
PROBE_PATH_RE = re.compile(
    r"(?i)(\.php\d?$|/wp-|wordpress|/\.env|/\.git|phpmyadmin|/cgi-bin|"
    r"/etc/passwd|\.\./|%2e%2e|/xmlrpc\.php|/actuator|/vendor/|/boaform|/hnap1)"
)
BOT_AGENT_RE = re.compile(r"(?i)(bot|crawler|spider|scan|curl/|python-requests|wget|masscan|zgrab)")

_EMAIL_RE = re.compile(r"[\w.+-]+@[\w-]+(?:\.[\w-]+)+")
_URL_RE = re.compile(r"(?i)\b(?:https?|wss?|ftp)://\S+")
_IPV4_RE = re.compile(r"\b\d{1,3}(?:\.\d{1,3}){3}\b")
_IPV6_RE = re.compile(r"(?i)(?<![\w:])(?:[0-9a-f]{0,4}:){2,7}[0-9a-f]{0,4}(?![\w:])")
_SECRET_KV_RE = re.compile(
    r"(?i)\b(pass(?:word|wd|phrase)?|pwd|secret|token|api[_-]?key|apikey|"
    r"session[_-]?id|sid|cookie|set-cookie|authorization|bearer|csrf[_-]?token|"
    r"signature|otp|private[_-]?key|access[_-]?key)\b(\s*[=:]\s*)([^\s,;&'\"]+)"
)
_BEARER_RE = re.compile(r"(?i)\bbearer\s+[\w.~+/=-]+")
_LONG_TOKEN_RE = re.compile(r"\b[\w+/=-]{24,}\b")
_HEX_RE = re.compile(r"(?i)\b[0-9a-f]{16,}\b")
_CALLSIGN_RE = re.compile(r"\b[A-Z]{1,2}\d[A-Z]{1,4}\b")
_LONG_DIGITS_RE = re.compile(r"\d{7,}")
_DIGIT_SEGMENT_RE = re.compile(r"\d")

# call_kw / call_button put "<model>.<method>" in the path: that is a code identifier and is the
# useful part of the route for an RPC error. They have no other variable parts.
_RPC_PREFIXES = ("/web/dataset/call_kw", "/web/dataset/call_button")


# [@ANCHOR: hams_helpdesk:exc_scrub_text]
def scrub_text(text, strict=True):
    """Removes emails, URLs (with their query strings), IP addresses, secret-looking key=value pairs,
    bearer tokens, hex/base64 tokens and, if ``strict``, callsigns and long digit runs. ``strict`` is
    for exception messages; source lines are code and only get the first group."""
    text = str(text or "")
    text = _URL_RE.sub("<url>", text)
    text = _EMAIL_RE.sub("<email>", text)
    text = _BEARER_RE.sub("bearer <redacted>", text)
    text = _SECRET_KV_RE.sub(lambda m: m.group(1) + m.group(2) + "<redacted>", text)
    text = _IPV4_RE.sub("<ip>", text)
    text = _IPV6_RE.sub("<ip>", text)
    if not strict:
        return text
    text = _HEX_RE.sub("<token>", text)
    text = _LONG_TOKEN_RE.sub("<token>", text)
    text = _CALLSIGN_RE.sub("<callsign>", text)
    return _LONG_DIGITS_RE.sub("<num>", text)


# [@ANCHOR: hams_helpdesk:exc_normalize_route]
def normalize_route(path, rule=None):
    """The route to report: the URL path only, never a query string, and never a variable part.

    ``rule`` is werkzeug's own pattern for the matched route ("/community/<string:slug>"). It is used
    whenever there is one, because the raw path of a profile page contains a callsign. RPC paths
    keep "<model>.<method>". With no matched rule (a fallback page) every path segment that holds a
    digit, or is long, is replaced."""
    path = str(path or "").split("?", 1)[0].split("#", 1)[0] or "/"
    if path.startswith(_RPC_PREFIXES):
        return scrub_text(path, strict=False)[:200]
    if rule:
        return str(rule)[:200]
    segments = []
    for segment in path.split("/"):
        if _DIGIT_SEGMENT_RE.search(segment) or len(segment) > 24:
            segment = "<x>"
        segments.append(segment)
    return scrub_text("/".join(segments), strict=False)[:200]


# [@ANCHOR: hams_helpdesk:exc_is_expected]
def is_expected(exc, source, logged_in, path="", user_agent=""):
    """True when ``exc`` is ordinary control flow or noise and must not become a ticket.

    ``source`` is "http" (a page controller), "rpc" (JSON-RPC, including every ORM call from the web
    client) or "cron". ``logged_in`` is whether the session is authenticated. Order matters: AccessError
    is a UserError, so it is decided before UserError."""
    if isinstance(exc, (BrokenPipeError, ConnectionResetError, werkzeug.exceptions.ClientDisconnected)):
        return True  # the browser went away
    if isinstance(exc, werkzeug.exceptions.HTTPException):
        # NotFound, redirects, CSRF (400), 403/405/413... all answered correctly already.
        return (exc.code or 0) < 500
    if isinstance(exc, exceptions.AccessDenied):
        return True  # a failed login
    if isinstance(exc, exceptions.AccessError):
        # A logged-in user failing to open a page that should open is a bug (the /community case).
        # A permission dialog in the web client, or an anonymous probe, is not.
        return not (source == "http" and logged_in)
    if isinstance(exc, exceptions.MissingError):
        return source != "cron"  # a deleted record; a 404 for a page, shown as a dialog over RPC
    if isinstance(exc, exceptions.RedirectWarning):
        return True
    if isinstance(exc, exceptions.UserError):
        # UserError / ValidationError are shown to the person who caused them; nobody sees a cron's.
        return source != "cron"
    if isinstance(exc, psycopg2.errors.TransactionRollbackError):
        return source == "cron"  # a retried serialization failure; Odoo runs the cron again
    if source != "cron" and not logged_in:
        if PROBE_PATH_RE.search(path or "") or BOT_AGENT_RE.search(user_agent or ""):
            return True
    return False


# [@ANCHOR: hams_helpdesk:exc_root_cause]
def root_cause(exc):
    """The exception that really failed. A server action or cron job's code is run through
    ``safe_eval``, which re-raises every error as a ``ValueError`` (the real one is its
    ``__context__``), and QWeb wraps a render error in ``QWebError``. Fingerprinting those
    wrappers would file every different failure of one job under ValueError in ``ir_actions``."""
    seen = set()
    while id(exc) not in seen:
        seen.add(id(exc))
        inner = exc.__cause__ or exc.__context__
        wrapped = isinstance(exc, QWebError) or _raised_in_safe_eval(exc)
        if inner is None or not wrapped:
            break
        exc = inner
    return exc


# [@ANCHOR: hams_helpdesk:exc_raised_in_safe_eval]
def _raised_in_safe_eval(exc):
    """True when the innermost frame of ``exc`` is inside odoo.tools.safe_eval."""
    innermost = None
    for frame, _lineno in traceback.walk_tb(exc.__traceback__):
        innermost = frame.f_globals.get("__name__")
    return innermost == "odoo.tools.safe_eval"


# [@ANCHOR: hams_helpdesk:exc_describe]
def describe(exc):
    """(type name, module, innermost-frame "module:line in function") for the fingerprint.

    The frame is the innermost one in an addon (``odoo.addons.<module>``), so a bug in a model method
    is not folded into every other bug that ends in odoo/orm/models.py. With no addon frame, the
    innermost frame overall is used and the module is "odoo". Taken from each frame's module name,
    not its file path, so the result is the same on the dev box and on the server."""
    exc = root_cause(exc)
    exc_type = type(exc)
    type_name = f"{exc_type.__module__}.{exc_type.__qualname__}"
    chosen = None
    module = "odoo"
    for frame, lineno in traceback.walk_tb(exc.__traceback__):
        name = frame.f_globals.get("__name__", "?")
        entry = (name, lineno, frame.f_code.co_name)
        if name.startswith("odoo.addons."):
            chosen = entry
            module = name.split(".")[2]
        elif chosen is None or not chosen[0].startswith("odoo.addons."):
            chosen = entry
    if chosen is None:
        return type_name, module, "<no frame>"
    return type_name, module, f"{chosen[0]}:{chosen[1]} in {chosen[2]}"


# [@ANCHOR: hams_helpdesk:exc_fingerprint]
def fingerprint(type_name, module, frame, route):
    """Stable 40-hex-character key for (exception type, innermost frame, module, route)."""
    key = "\x1f".join((type_name, frame, module, route))
    return hashlib.sha256(key.encode("utf-8")).hexdigest()[:40]


# [@ANCHOR: hams_helpdesk:exc_frame_lines]
def _frame_lines(exc):
    """Scrubbed "module:line in function / source" lines for one exception's own traceback."""
    out = []
    for frame, lineno in traceback.walk_tb(exc.__traceback__):
        name = frame.f_globals.get("__name__", "?")
        out.append(f"  {name}:{lineno} in {frame.f_code.co_name}")
        source = linecache.getline(frame.f_code.co_filename, lineno).strip()
        if source:
            out.append("    " + scrub_text(source[:CODE_LINE_LIMIT], strict=False))
    return out[-2 * MAX_FRAMES:]


# [@ANCHOR: hams_helpdesk:exc_scrubbed_trace]
def scrubbed_trace(exc):
    """The exception chain (``__cause__`` / ``__context__``, at most MAX_CHAIN deep) as text that is
    safe to store. Built from the frames, never from ``traceback.format_exception``, so no local
    variable, argument value or full message is ever read."""
    parts = []
    seen = set()
    current = exc
    while current is not None and id(current) not in seen and len(parts) < MAX_CHAIN:
        seen.add(id(current))
        exc_type = type(current)
        head = f"{exc_type.__module__}.{exc_type.__qualname__}"
        if isinstance(current, MESSAGE_SAFE_TYPES):
            message = scrub_text(str(current).replace("\n", " "))[:MESSAGE_LIMIT]
            head += f": {message}"
        elif isinstance(current, psycopg2.Error) and current.pgcode:
            head += f" (pgcode {current.pgcode})"
        else:
            head += ": <message withheld>"
        parts.append("\n".join([head] + _frame_lines(current)))
        current = current.__cause__ or (None if current.__suppress_context__ else current.__context__)
    return "\n--- caused by / during handling of ---\n".join(reversed(parts))


# Failure types Odoo records for a recipient's or a sender's data rather than for a broken mail
# system: a mistyped address, a blacklisted or opted-out recipient, a duplicate. They are shown on
# the mail itself and are not tickets.
MAIL_FAILURES_NOT_REPORTED = frozenset(
    ("mail_email_invalid", "mail_email_missing", "mail_bl", "mail_optout", "mail_dup")
)
# (class, pattern) pairs tried in order against the transport's failure text. Only the class label
# ever leaves ``mail_failure_class``; the text itself is never stored.
_MAIL_FAILURE_PATTERNS = (
    ("auth", r"authenticat|smtpauth|\b535\b|credentials|username and password"),
    ("tls", r"\bssl\b|\btls\b|certificate|starttls"),
    ("timeout", r"timed out|timeout"),
    ("connect", (
        r"connection (?:refused|reset|aborted|closed)|name or service|getaddrinfo|unreachable|"
        r"unable to connect|disconnected|network is|no route"
    )),
    ("temporary_4xx", r"\b4\d\d\b"),
    ("permanent_5xx", r"\b5\d\d\b"),
)
# Anchored with a leading ".*?" so ``match`` finds the pattern anywhere in the text.
_MAIL_FAILURE_CLASSES = tuple(
    (label, re.compile(r".*?(?:" + pattern + ")", re.IGNORECASE | re.DOTALL))
    for label, pattern in _MAIL_FAILURE_PATTERNS
)


# [@ANCHOR: hams_helpdesk:exc_mail_failure_class]
def mail_failure_class(reason):
    """A label from a fixed vocabulary for the transport's failure text; "other" when nothing matches.
    The text can hold an address, a subject or a server banner, so none of it is returned."""
    text = str(reason or "")
    for label, pattern in _MAIL_FAILURE_CLASSES:
        if pattern.match(text):
            return label
    return "other"


DAEMON_KINDS = ("unit_failed", "consumer_exception")
DAEMON_RESULTS = frozenset(
    ("exit-code", "signal", "core-dump", "timeout", "watchdog", "start-limit-hit", "resources", "oom-kill")
)
DAEMON_TAIL_LINES = 20
DAEMON_TAIL_LINE_LIMIT = 160
DAEMON_TAIL_LIMIT = 4000
_DAEMON_UNIT_SUFFIXES = (".service", ".timer", ".socket", ".target")
_DAEMON_UNIT_RE = re.compile(r"^[A-Za-z0-9][A-Za-z0-9_.:@-]{0,79}$")
_DAEMON_EXC_LINE_RE = re.compile(r"^((?:[A-Za-z_][\w.]*\.)?[A-Z]\w*(?:Error|Exception|Exit|Interrupt|Warning)):.*$")


# [@ANCHOR: hams_helpdesk:exc_daemon_unit_name]
def daemon_unit_name(unit):
    """The systemd unit name to report, or None when ``unit`` is not a plain unit name. The instance
    part of a template unit ("worker@<instance>.service") is dropped: it can be a user or a path.
    Names come from the operator's own unit files, so anything but a short plain name is refused."""
    text = str(unit or "").strip()
    head, at_sign, tail = text.partition("@")
    if at_sign:
        suffix = tail.rsplit(".", 1)[1] if "." in tail else ""
        text = f"{head}@.{suffix}" if suffix else f"{head}@"
    if not _DAEMON_UNIT_RE.match(text) or not text.endswith(_DAEMON_UNIT_SUFFIXES):
        return None
    return text


# [@ANCHOR: hams_helpdesk:exc_daemon_tail]
def scrub_daemon_tail(text):
    """The last lines of a daemon's journal, safe to store. Free text from outside this codebase, so
    the strict scrubber is applied to every line (emails, URLs, IP addresses, secret pairs, tokens,
    callsigns, long digit runs), an exception line keeps only its class (the message is dropped, as for
    a Python exception in ``scrubbed_trace``), and the result is cut to a fixed number and length of
    lines."""
    lines = [line.rstrip() for line in str(text or "").replace("\r", "").split("\n") if line.strip()]
    out = []
    for line in lines[-DAEMON_TAIL_LINES:]:
        line = line.strip()
        exc_line = _DAEMON_EXC_LINE_RE.match(line)
        if exc_line:
            line = exc_line.group(1) + ": <message withheld>"
        out.append(scrub_text(line[:DAEMON_TAIL_LINE_LIMIT]))
    return "\n".join(out)[:DAEMON_TAIL_LIMIT]
