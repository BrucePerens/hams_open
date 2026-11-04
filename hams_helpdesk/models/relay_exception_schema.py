# Copyright © Bruce Perens K6BP.
# SPDX-License-Identifier: AGPL-3.0-or-later
"""Pure validation for the exception and crash reports a hams_local_relay sends to hams.com.

The relay sends the report as the parameters of an ``exception_report`` call on its authenticated uplink
(``ham_relay_bridge``, hams_com). The relay is authenticated, but its software runs on a machine we do not control, so
the report is still untrusted data: it is parsed strictly (size, a fixed set of keys, a fixed type for each),
every stored value is rebuilt from a validated piece, and message, frames and log lines are scrubbed AGAIN
here with ``exception_scrub`` (the relay scrubs first; a relay that is old, broken or hostile does not get to
put secrets into a ticket).

Nothing here touches the database, Redis or the request, so every rule can be tested with plain inputs.
What a report can carry, and nothing else: the kind (panic, signal, error), a message, the signal name, a
thread name, up to MAX_FRAMES frames as function paths, the relay version and build, the platform, the uptime, a
repeat count, the time, up to MAX_LOG_LINES scrubbed log lines and the key of the operator who was acting. It
never carries audio, radio traffic, a node id or an account id: the relay's identity and its owner come from
the authenticated link.
"""

import collections
import json
import re
import time

from . import exception_scrub as scrub

# [@ANCHOR: hams_helpdesk:relay_limits]
MAX_BODY_BYTES = 16384
MAX_FRAMES = 25
MAX_LOG_LINES = 20
MAX_MESSAGE_CHARS = 500
MAX_THREAD_CHARS = 64
MAX_FRAME_CHARS = 240
MAX_LOG_CHARS = 240
STORED_MESSAGE_CHARS = 300
MAX_COUNT = 1_000_000
MAX_UPTIME_SECONDS = 10**9
MAX_TIME = 4102444800  # 2100-01-01
SCHEMA_VERSION = 1
KINDS = ("panic", "signal", "error")
# Fatal signals only. SIGTERM and SIGKILL are a shutdown asked for, not a crash.
SIGNALS = ("SIGSEGV", "SIGBUS", "SIGILL", "SIGFPE", "SIGABRT")
# [@ANCHOR: hams_helpdesk:relay_report_wire]
KEYS = frozenset(
    (
        "v", "kind", "message", "signal", "thread", "frames", "version", "build", "os", "arch",
        "uptime_s", "count", "occurred_at", "log_tail", "operator_key",
    )
)
OPTIONAL_KEYS = frozenset(("unsymbolised",))
# The ticket title prefixes (Bruce, 2026-11-03: "Relay"). One constant each, used by the pipeline and the tests.
TITLE_PREFIX_EXCEPTION = "Relay exception: "
TITLE_PREFIX_CRASH = "Relay crash: "
SOURCE = "relay"
CRATE = "hams_local_relay"

VERSION_RE = re.compile(r"^[0-9A-Za-z][0-9A-Za-z.+_-]{0,31}$")
BUILD_RE = re.compile(r"^[0-9A-Za-z._-]{1,64}$")
OS_RE = re.compile(r"^[0-9A-Za-z._ -]{1,32}$")
ARCH_RE = re.compile(r"^[0-9A-Za-z._-]{1,16}$")
OPERATOR_KEY_RE = re.compile(r"^(?:[0-9a-f]{64,130})?$")
# A frame is a function path, optionally followed by " (file:line)" or an offset; never free text.
FRAME_RE = re.compile(r"^[^\s\x00-\x1f][^\x00-\x1f]{0,239}$")
# The relay id comes from the authenticated link (a hash of its key), not from the report; it is kept to a plain alphabet and not scrubbed as a token.
_RELAY_ID_BAD_RE = re.compile(r"[^0-9A-Za-z_.:-]")
_CONTROL_RE = re.compile(r"[\x00-\x08\x0b-\x1f\x7f]")
_LOCATION_RE = re.compile(r"\s*\([^()]*:\d+(?::\d+)?\)\s*$")
_HASH_RE = re.compile(r"::h[0-9a-f]{16}$")
_CLOSURE_RE = re.compile(r"\{\{?closure(?:#\d+)?\}\}?")
_GENERIC_RE = re.compile(r"(?<=[\w}])<(?:[^<>]|<>)*>")
_ADDRESS_RE = re.compile(r"\s*\+?0x[0-9a-fA-F]+")
# Frames that are the runtime's, not the relay's: a crash is blamed on the first frame that is not one of these.
RUNTIME_PREFIXES = (
    "std::", "core::", "alloc::", "backtrace::", "tokio::", "futures", "rust_begin_unwind", "__rust", "rust_panic",
    "<unresolved>", "__libc", "libc::", "_start", "start_thread", "clone", "__restore_rt", "hams_local_relay::crash_report::",
)

# [@ANCHOR: hams_helpdesk:relay_report_tuple]
Report = collections.namedtuple(
    "Report", "type_name module frame route trace name kind signal count operator_key occurred_at key_parts"
)


# [@ANCHOR: hams_helpdesk:relay_report_rejected]
class ReportRejected(Exception):
    """The report is refused. ``status`` is an HTTP-like code and ``reason`` a short fixed word (never a piece of
    the report) for the log and the tests."""

    # [@ANCHOR: hams_helpdesk:relay_report_rejected_init]
    def __init__(self, status, reason):
        super().__init__(reason)
        self.status = status
        self.reason = reason


# [@ANCHOR: hams_helpdesk:relay_is_int]
def _is_int(value):
    return isinstance(value, int) and not isinstance(value, bool)


# [@ANCHOR: hams_helpdesk:relay_clean]
def _clean(text):
    """``text`` with control characters other than a tab or newline replaced by a space."""
    return _CONTROL_RE.sub(" ", text)


# [@ANCHOR: hams_helpdesk:relay_parse_body]
def parse_body(raw):
    """The validated report object in ``raw`` (bytes): at most MAX_BODY_BYTES, UTF-8 JSON, no NUL byte
    (PostgreSQL cannot store one), an object. Then ``validate_report`` checks its keys and values.
    Raises ``ReportRejected``."""
    if len(raw) > MAX_BODY_BYTES:
        raise ReportRejected(413, "too_large")
    if b"\x00" in raw or "\\u0000" in raw.decode("utf-8", "replace"):
        raise ReportRejected(400, "nul_byte")
    try:
        data = json.loads(raw.decode("utf-8"))
    except (UnicodeDecodeError, ValueError, RecursionError) as error:
        raise ReportRejected(400, "not_json") from error
    return validate_report(data)


# [@ANCHOR: hams_helpdesk:relay_validate_report]
def validate_report(data):
    """``data`` (the parsed report) when every key and value is well-formed, else raises ``ReportRejected``.
    Does not change ``data``. A parameters dict that arrives already parsed (the relay-call route) is re-encoded
    first so the same size limit applies."""
    if not isinstance(data, dict):
        raise ReportRejected(400, "not_an_object")
    if len(json.dumps(data, ensure_ascii=False).encode("utf-8")) > MAX_BODY_BYTES:
        raise ReportRejected(413, "too_large")
    keys = set(data)
    if not KEYS <= keys or not keys <= KEYS | OPTIONAL_KEYS:
        raise ReportRejected(400, "wrong_keys")
    if not _is_int(data["v"]) or data["v"] != SCHEMA_VERSION:
        raise ReportRejected(400, "wrong_version")
    for key in ("kind", "message", "signal", "thread", "version", "build", "os", "arch", "operator_key"):
        if not isinstance(data[key], str):
            raise ReportRejected(400, "wrong_type")
    for key in ("uptime_s", "count", "occurred_at"):
        if not _is_int(data[key]):
            raise ReportRejected(400, "wrong_type")
    if "unsymbolised" in data and not isinstance(data["unsymbolised"], bool):
        raise ReportRejected(400, "wrong_type")
    if data["kind"] not in KINDS:
        raise ReportRejected(400, "wrong_kind")
    if data["kind"] == "signal":
        if data["signal"] not in SIGNALS:
            raise ReportRejected(400, "wrong_signal")
    elif data["signal"] != "":
        raise ReportRejected(400, "wrong_signal")
    if len(data["message"]) > MAX_MESSAGE_CHARS or len(data["thread"]) > MAX_THREAD_CHARS:
        raise ReportRejected(400, "too_long")
    for key, pattern in (("version", VERSION_RE), ("build", BUILD_RE), ("os", OS_RE), ("arch", ARCH_RE)):
        if not pattern.match(data[key]):
            raise ReportRejected(400, f"wrong_{key}")
    if not OPERATOR_KEY_RE.match(data["operator_key"]):
        raise ReportRejected(400, "wrong_operator_key")
    if not 0 <= data["uptime_s"] <= MAX_UPTIME_SECONDS:
        raise ReportRejected(400, "wrong_uptime")
    if not 1 <= data["count"] <= MAX_COUNT:
        raise ReportRejected(400, "wrong_count")
    if not 0 < data["occurred_at"] <= MAX_TIME:
        raise ReportRejected(400, "wrong_time")
    frames = data["frames"]
    if not isinstance(frames, list) or not 1 <= len(frames) <= MAX_FRAMES:
        raise ReportRejected(400, "wrong_frames")
    if not all(isinstance(frame, str) and 0 < len(frame) <= MAX_FRAME_CHARS and FRAME_RE.match(frame) for frame in frames):
        raise ReportRejected(400, "wrong_frames")
    lines = data["log_tail"]
    if not isinstance(lines, list) or len(lines) > MAX_LOG_LINES:
        raise ReportRejected(400, "wrong_log_tail")
    if not all(isinstance(line, str) and len(line) <= MAX_LOG_CHARS for line in lines):
        raise ReportRejected(400, "wrong_log_tail")
    return data


# [@ANCHOR: hams_helpdesk:relay_scrub_line]
def scrub_line(text, limit):
    """One message or log line as it may be stored: control characters removed, then ``scrub_text`` (emails, URLs,
    addresses, secret key=value pairs, bearer tokens, long tokens, callsigns, long digit runs), then cut to ``limit``."""
    return scrub.scrub_text(_clean(text).replace("\n", " "))[:limit]


# [@ANCHOR: hams_helpdesk:relay_normalize_frame]
def normalize_frame(frame):
    """The frame as the fingerprint sees it: the function path only. Drops the ``(file:line)`` location, a ``+0x``
    offset or address, the compiler's ``::h<16 hex>`` hash, closure numbers (``{closure#3}`` becomes
    ``{closure}``) and the contents of generic arguments after a name, so the same function in two builds
    has the same text. An unresolved frame is ``<unresolved>``."""
    text = _clean(frame).strip()
    text = _LOCATION_RE.sub("", text)
    text = _ADDRESS_RE.sub("", text)
    text = _HASH_RE.sub("", text)
    text = _CLOSURE_RE.sub("{closure}", text)
    while True:
        shorter = _GENERIC_RE.sub("<>", text)
        if shorter == text:
            break
        text = shorter
    text = text.strip()
    return text[:200] or "<unresolved>"


# [@ANCHOR: hams_helpdesk:relay_path_of]
def _path_of(normalized):
    return normalized[1:] if normalized.startswith("<") and not normalized.startswith("<unresolved") else normalized


# [@ANCHOR: hams_helpdesk:relay_top_frame]
def top_frame(frames):
    """``(function path, module)`` of the frame to blame. The first frame in the relay's own crate
    (``hams_local_relay::``), else the first that is not the runtime's (std, core, tokio, signal trampoline), else
    the first. ``module`` is the first path component after the crate name, or the first component of another path."""
    normalized = [normalize_frame(frame) for frame in frames]
    chosen = None
    for path in normalized:
        if _path_of(path).startswith(CRATE + "::") and not _path_of(path).startswith(CRATE + "::crash_report::"):
            chosen = path
            break
    if chosen is None:
        for path in normalized:
            if not _path_of(path).startswith(RUNTIME_PREFIXES):
                chosen = path
                break
    if chosen is None:
        chosen = normalized[0]
    parts = _path_of(chosen).split("::")
    module = parts[1] if parts[0] == CRATE and len(parts) > 1 else parts[0]
    return chosen, (module or CRATE)[:80]


# [@ANCHOR: hams_helpdesk:relay_function_label]
def _function_label(path):
    """The path without the crate name, for a ticket title (``multiplexer::handle_command``)."""
    text = _path_of(path)
    return text[len(CRATE) + 2:] if text.startswith(CRATE + "::") else text


# [@ANCHOR: hams_helpdesk:relay_build_report]
def build_report(data, relay_id="unknown", now=None):
    """The ``Report`` for the validated report ``data``. ``relay_id`` is the relay's id from the authenticated link
    (put into the trace text only; it is not part of the fingerprint). Scrubs the message, the frames and the log
    lines again. The fingerprint inputs are ``key_parts``: ``("relay.<kind>[.<signal>]", module, normalised
    top frame, "relay")``, which do not depend on the build, an address, a line number, the message, the
    thread or the count, so one crash in two builds is one ticket with a counter."""
    validate_report(data)
    kind = data["kind"]
    signal = data["signal"]
    type_name = f"{SOURCE}.{kind}" + (f".{signal}" if signal else "")
    frame, module = top_frame(data["frames"])
    function = _function_label(frame)
    verb = {"panic": "panic", "signal": signal, "error": "error"}[kind]
    name = f"{verb} in {function}"[:200]
    message = scrub_line(data["message"], STORED_MESSAGE_CHARS)
    stamp = time.strftime("%Y-%m-%d %H:%M:%S", time.gmtime(min(data["occurred_at"], int(now or time.time()) + 86400)))
    frames = [scrub.scrub_text(_clean(f), strict=False)[:MAX_FRAME_CHARS] for f in data["frames"]]
    lines = [scrub_line(line, MAX_LOG_CHARS) for line in data["log_tail"]]
    header = [
        f"Relay report: {kind}" + (f" {signal}" if signal else "") + f": {message}",
        f"Relay id: {_RELAY_ID_BAD_RE.sub('?', str(relay_id))[:80]}",
        f"Relay version: {data['version']} build {data['build']} on {data['os']} {data['arch']}",
        f"Thread: {scrub_line(data['thread'], MAX_THREAD_CHARS)}",
        f"Uptime: {data['uptime_s']} s; occurrences since its last report: {data['count']}; at {stamp} UTC",
    ]
    if data.get("unsymbolised"):
        header.append("The build has no symbols: frames are addresses and offsets.")
    trace = "\n".join(
        header
        + ["Stack (innermost first):"]
        + [f"  {text}" for text in frames]
        + (["Last log lines (scrubbed):"] + [f"  {line}" for line in lines] if lines else [])
    )
    return Report(
        type_name,
        module,
        frame,
        SOURCE,
        trace,
        name,
        kind,
        signal,
        data["count"],
        data["operator_key"],
        data["occurred_at"],
        (type_name, module, frame, SOURCE),
    )
