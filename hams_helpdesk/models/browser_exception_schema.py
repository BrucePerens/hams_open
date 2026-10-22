# Copyright © Bruce Perens K6BP.
# SPDX-License-Identifier: AGPL-3.0-or-later
"""Pure validation for the browser JavaScript error reports (see controllers/browser_exception_api.py).

Nothing here touches the database, Redis or the request, so every rule can be tested with plain
inputs. The report is untrusted: a browser extension, a scanner or anybody with curl can post to the
endpoint. So the body is parsed strictly (size, JSON object, a fixed set of keys, a fixed type for
each), every stored value is rebuilt from a validated piece instead of copied, and a report is
refused unless its top stack frame is one of this site's own script files.

What a report can carry, and nothing else: an error class name, a message (kept only for the
JavaScript built-in error classes, scrubbed), up to MAX_FRAMES frames reduced to ``path:line:col``, a
route with its variable parts replaced, a browser family from a fixed list and the asset bundle
version. It never carries form values, page text, URLs with query strings, cookies or a user.
"""

import collections
import json
import re
from urllib.parse import urlsplit

from . import exception_scrub as scrub
from .exception_scrub import BOT_AGENT_RE

MAX_BODY_BYTES = 8192
MAX_FRAMES = 8
MAX_MESSAGE_CHARS = 1000
STORED_MESSAGE_CHARS = 300
MAX_FRAME_CHARS = 300
MAX_ROUTE_CHARS = 300
SCHEMA_VERSION = 1
KEYS = frozenset(("v", "type", "message", "frames", "route", "agent", "release"))
AGENT_FAMILIES = frozenset(("Firefox", "Chrome", "Edge", "Safari", "Opera", "Other"))

# JavaScript's own error classes build their messages from code ("Cannot read properties of null
# (reading 'x')"). Any other class (Error, a library's own, a rejection value) is free text that
# can carry what a person typed, so its message is withheld, as for a Python exception.
MESSAGE_SAFE_TYPES = frozenset(("TypeError", "ReferenceError", "RangeError", "SyntaxError", "EvalError", "URIError"))
TYPE_RE = re.compile(r"^[A-Za-z][A-Za-z0-9_$]{0,39}$")

# Errors that are not bugs in the page: Odoo's own RPC errors (the server already made a ticket for
# the ones that matter), a dropped connection, a user navigating away, the benign ResizeObserver
# notice, and the cross-origin "Script error." that carries no information.
IGNORED_TYPES = frozenset(
    ("RPCError", "RPC_ERROR", "ConnectionLostError", "ConnectionAbortedError", "AbortError", "NetworkError", "TimeoutError")
)
IGNORED_MESSAGE_RE = re.compile(
    r"(?i)(ResizeObserver loop|^script error\.?$|failed to fetch|load failed|networkerror when attempting|"
    r"the operation was aborted|the user aborted a request|network request failed|fetch is aborted)"
)

FRAME_RE = re.compile(r"^(?P<url>.+):(?P<line>\d{1,7}):(?P<col>\d{1,7})$")
ASSET_PATH_RE = re.compile(r"^/web/assets/(?:\d+/)?(?P<version>[0-9a-f]{7,64})/(?P<bundle>[A-Za-z0-9_.]+)\.min\.js$")
STATIC_PATH_RE = re.compile(r"^/(?P<module>[a-z_][a-z0-9_]{0,63})/static/(?P<rest>[A-Za-z0-9_./-]{1,200}\.js)$")
RELEASE_RE = re.compile(r"^(?P<bundle>[A-Za-z0-9_.]{3,80})@(?P<version>[0-9a-f]{7,64})$")
# The bundles the reporter is shipped in, and the other frontend ones a page loads beside them.
KNOWN_BUNDLES = ("web.assets_frontend", "web.assets_frontend_minimal", "web.assets_frontend_lazy", "web.assets_backend")

Frame = collections.namedtuple("Frame", "path line col module bundle version")
Report = collections.namedtuple("Report", "type_name module frame route trace name release bundle version agent")


class ReportRejected(Exception):
    """The report is refused. ``status`` is the HTTP status to answer, ``reason`` a short fixed word
    (never a piece of the report) for the log and the tests."""

    def __init__(self, status, reason):
        super().__init__(reason)
        self.status = status
        self.reason = reason


# [@ANCHOR: hams_helpdesk:browser_parse_body]
def parse_body(raw):
    """The report object in ``raw`` (bytes): at most MAX_BODY_BYTES, UTF-8 JSON, no NUL byte (PostgreSQL
    cannot store one), an object with exactly the known keys. Raises ``ReportRejected``."""
    if len(raw) > MAX_BODY_BYTES:
        raise ReportRejected(413, "too_large")
    if b"\x00" in raw or "\\u0000" in raw.decode("utf-8", "replace"):
        raise ReportRejected(400, "nul_byte")
    try:
        data = json.loads(raw.decode("utf-8"))
    except (UnicodeDecodeError, ValueError, RecursionError) as error:
        raise ReportRejected(400, "not_json") from error
    if not isinstance(data, dict):
        raise ReportRejected(400, "not_an_object")
    if set(data) != KEYS:
        raise ReportRejected(400, "wrong_keys")
    if data["v"] != SCHEMA_VERSION or isinstance(data["v"], bool):
        raise ReportRejected(400, "wrong_version")
    for key in ("type", "message", "route", "agent", "release"):
        if not isinstance(data[key], str):
            raise ReportRejected(400, "wrong_type")
    frames = data["frames"]
    if not isinstance(frames, list) or not 1 <= len(frames) <= MAX_FRAMES:
        raise ReportRejected(400, "wrong_frames")
    if not all(isinstance(frame, str) and 0 < len(frame) <= MAX_FRAME_CHARS for frame in frames):
        raise ReportRejected(400, "wrong_frames")
    if len(data["message"]) > MAX_MESSAGE_CHARS or len(data["route"]) > MAX_ROUTE_CHARS:
        raise ReportRejected(400, "too_long")
    return data


# [@ANCHOR: hams_helpdesk:browser_parse_frame]
def parse_frame(text, own_host=None):
    """``Frame`` for one ``[origin]path:line:col`` string, or raises ``ReportRejected(422, "foreign_script")``
    when it is not one of this site's own script files.

    A frame with an origin must be on ``own_host`` (the host the report was posted to); a bare path is
    accepted as it is. The path must be an Odoo asset bundle (``/web/assets/<version>/<bundle>.min.js``)
    or a module's ``static`` script, with no query string, fragment or ``..``. A browser extension's
    ``chrome-extension://`` file, another site's script and a ``blob:`` or ``data:`` URL all fail here."""
    match = FRAME_RE.match(text)
    if not match:
        raise ReportRejected(400, "bad_frame")
    url = match.group("url")
    if "?" in url or "#" in url or ".." in url or "\\" in url or any(ord(c) < 32 for c in url):
        raise ReportRejected(400, "bad_frame")
    if url.startswith("/"):
        path = url
    else:
        parts = urlsplit(url)
        if parts.scheme not in ("http", "https") or parts.username or parts.password:
            raise ReportRejected(422, "foreign_script")
        if not own_host or parts.netloc.lower() != own_host.lower():
            raise ReportRejected(422, "foreign_script")
        path = parts.path
    asset = ASSET_PATH_RE.match(path)
    if asset:
        return Frame(path, int(match.group("line")), int(match.group("col")), asset.group("bundle"), asset.group("bundle"), asset.group("version"))
    static = STATIC_PATH_RE.match(path)
    if static:
        return Frame(path, int(match.group("line")), int(match.group("col")), static.group("module"), None, None)
    raise ReportRejected(422, "foreign_script")


# [@ANCHOR: hams_helpdesk:browser_frame_key]
def frame_key(frame):
    """The frame as the fingerprint sees it: the bundle's version is dropped from the path (it changes
    with every release) and the line and column kept."""
    path = frame.path
    if frame.version:
        path = f"/web/assets/*/{frame.bundle}.min.js"
    return f"{path}:{frame.line}:{frame.col}"


# [@ANCHOR: hams_helpdesk:browser_is_bot_agent]
def is_bot_agent(user_agent):
    """True when the request's User-Agent header is a crawler, scanner or command-line client (the list the
    server-exception reporter already uses). A real visitor's browser reports; a program does not."""
    return BOT_AGENT_RE.search(user_agent or "") is not None


# [@ANCHOR: hams_helpdesk:browser_agent_family]
def agent_family(value):
    """``value`` when it is one of the fixed browser families, else raises ``ReportRejected``."""
    if value not in AGENT_FAMILIES:
        raise ReportRejected(400, "wrong_agent")
    return value


# [@ANCHOR: hams_helpdesk:browser_is_ignorable]
def is_ignorable(type_name, message):
    """True for the errors that are not bugs in the page (see IGNORED_TYPES and IGNORED_MESSAGE_RE)."""
    return type_name in IGNORED_TYPES or bool(IGNORED_MESSAGE_RE.search(message or ""))


# [@ANCHOR: hams_helpdesk:browser_stored_message]
def stored_message(type_name, message):
    """The message to store: scrubbed and cut for a JavaScript built-in error class, else a marker."""
    if type_name not in MESSAGE_SAFE_TYPES:
        return "<message withheld>"
    return scrub.scrub_text(message.replace("\n", " "))[:STORED_MESSAGE_CHARS]


# [@ANCHOR: hams_helpdesk:browser_build_report]
def build_report(data, own_host=None, bot_agent=False):
    """The validated ``Report`` for the parsed body ``data``, or raises ``ReportRejected``.

    Order: type name and release shape, then ignorable noise (422 ``ignored``), then the frames (the top
    one must be this site's own script; a later foreign frame, an extension's wrapper say, is dropped),
    then the route. ``bot_agent`` (the request's User-Agent matched the bot list) refuses the report."""
    type_name = data["type"]
    if not TYPE_RE.match(type_name):
        raise ReportRejected(400, "wrong_type_name")
    release = RELEASE_RE.match(data["release"])
    if not release:
        raise ReportRejected(400, "wrong_release")
    agent = agent_family(data["agent"])
    route = data["route"]
    if not route.startswith("/") or "?" in route or "#" in route:
        raise ReportRejected(400, "wrong_route")
    if bot_agent:
        raise ReportRejected(422, "bot")
    if is_ignorable(type_name, data["message"]):
        raise ReportRejected(204, "ignored")
    frames = [parse_frame(data["frames"][0], own_host)]
    for text in data["frames"][1:]:
        try:
            frames.append(parse_frame(text, own_host))
        except ReportRejected as error:
            if error.reason != "foreign_script":
                raise
    top = frames[0]
    if top.version and top.version != release.group("version"):
        raise ReportRejected(422, "release_mismatch")
    route = scrub.normalize_route(route)
    message = stored_message(type_name, data["message"])
    trace = "\n".join(
        [
            f"Browser error: {type_name}: {message}",
            f"Route: {route}",
            f"Browser: {agent}",
            f"Asset bundle: {data['release']}",
            "Stack (innermost first, script path:line:col):",
        ]
        + [f"  {frame.path}:{frame.line}:{frame.col}" for frame in frames]
    )
    return Report(
        f"browser.{type_name}",
        top.module,
        frame_key(top),
        route,
        trace,
        f"{type_name} in {top.module} at {route}",
        data["release"],
        release.group("bundle"),
        release.group("version"),
        agent,
    )
