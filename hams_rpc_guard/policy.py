# -*- coding: utf-8 -*-
# Copyright © Bruce Perens K6BP.
# SPDX-License-Identifier: AGPL-3.0-or-later
"""Who may call which model method over RPC: the decision, and the counters that make report-only mode useful.

The decision is a pure function of facts about the call (`Call`) and the declarations (`exposure.Exposure`), so it is
tested without a database. `patch.py` gathers the facts from a live request and applies the verdict.

Rules, in order, for a call that arrives over `/web/dataset/call_kw`, `/web/dataset/call_button` or an external RPC
endpoint:

1. A **public or portal** user (strict mode): allowed only if the pair is declared `portal`. This covers stock models too:
   a portal visitor's browser has no business calling an arbitrary stock method.
2. A **service account whose login (or verified client-certificate subject, `cert:<subject>`) has a `clients` entry**
   (a daemon): allowed only if the pair is in that entry, or in both when both have one. A daemon therefore cannot
   call the methods of another daemon's family. A service account without an entry is
   treated as an internal user (and counted as `unmapped-client` in the report so it can be given one).
3. An **internal user**: a method defined by one of *our* addons (a managed module) is allowed only if the pair is
   declared `internal`, or carries `@public_rpc`. A method defined by stock Odoo, or a standard ORM override
   (`write`, `search_read`, `web_read`, ...), keeps core behaviour, unless `strict_stock` is on, when it must be
   declared `stock` or `internal`.

`mode` is `enforce` (refuse with an access error), `report` (count and log, allow) or `off`.
"""
import collections
import logging
import threading

from odoo import _
from odoo.exceptions import AccessError

from .endpoint_gate import is_power_of_ten

_logger = logging.getLogger(__name__)

MODE_ENFORCE = "enforce"
MODE_REPORT = "report"
MODE_OFF = "off"
MODES = (MODE_ENFORCE, MODE_REPORT, MODE_OFF)
DEFAULT_MODE = MODE_ENFORCE

MODE_PARAM = "hams_rpc_guard.mode"
STRICT_STOCK_PARAM = "hams_rpc_guard.strict_stock"
MANAGED_AUTHORS_PARAM = "hams_rpc_guard.managed_authors"
CLIENT_CERT_HEADER_PARAM = "hams_rpc_guard.client_cert_header"
DEFAULT_MANAGED_AUTHORS = ("Bruce Perens", "Hams", "HAMS")

USER_PUBLIC = "public"
USER_PORTAL = "portal"
USER_SERVICE = "service"
USER_INTERNAL = "internal"

ALLOW = "allow"
DENY = "deny"

_counts = collections.Counter()
_counts_lock = threading.Lock()


class Call:
    """The facts about one RPC call. Plain attributes so a test can build one without Odoo.

    `model` and `method` are names; `user_class` one of the USER_ constants; `login` the caller's login when it is a
    service account; `defining_models` the `_name` of every class that defines the method (a mixin's name, the
    concrete model's), `managed` whether the first class that defines it belongs to one of our addons, `orm_standard`
    whether the name is a standard ORM method, `marks` the `@public_rpc` marks found on it."""

    # [@ANCHOR: hams_rpc_guard:call_init]
    def __init__(self, model, method, user_class, login="", defining_models=(), managed=False, orm_standard=False,
                 marks=(), cert=""):
        self.model = model
        self.method = method
        self.user_class = user_class
        self.login = login
        self.defining_models = tuple(defining_models)
        self.managed = managed
        self.orm_standard = orm_standard
        self.marks = tuple(marks)
        self.cert = cert


# [@ANCHOR: hams_rpc_guard:declared]
def _declared(pairs, call):
    """Whether the call's pair, under the model's own name or any defining class's name, is in `pairs`."""
    return any((name, call.method) in pairs for name in (call.model,) + call.defining_models)


# [@ANCHOR: hams_rpc_guard:decide]
def decide(call, exposure, strict_stock=False):
    """The verdict on one call: `(ALLOW | DENY, reason)`. `reason` is a short constant for the counters."""
    if call.user_class in (USER_PUBLIC, USER_PORTAL):
        if _declared(exposure.portal, call) or any(mark["portal"] for mark in call.marks):
            return ALLOW, "portal-listed"
        return DENY, "strict-" + call.user_class
    if call.user_class == USER_SERVICE:
        # The verified certificate subject (`cert:<subject>`) and the login each narrow the daemon: when both have an
        # entry the pair must be in both, so claiming another daemon's certificate can never widen what a login may do.
        entries = [identity for identity in (call.cert, call.login) if identity and identity in exposure.clients]
        if entries:
            if all(_declared(exposure.clients[identity], call) or any(identity in mark["clients"] for mark in call.marks)
                   for identity in entries):
                return ALLOW, "client-listed"
            return DENY, "client-not-listed"
    reason_prefix = "unmapped-client-" if call.user_class == USER_SERVICE else ""
    if call.managed and not call.orm_standard:
        if call.marks or _declared(exposure.internal, call):
            return ALLOW, reason_prefix + "listed"
        return DENY, reason_prefix + "managed-not-listed"
    if strict_stock and not call.managed:
        if call.marks or _declared(exposure.stock | exposure.internal, call):
            return ALLOW, "stock-listed"
        return DENY, "strict-stock"
    return ALLOW, reason_prefix + "core"


# [@ANCHOR: hams_rpc_guard:normalise_mode]
def normalise_mode(value):
    """`value` as a mode: unset takes `enforce`; anything unrecognised also takes `enforce` (fail closed)."""
    value = str(value or "").strip().lower()
    return value if value in MODES else DEFAULT_MODE


# [@ANCHOR: hams_rpc_guard:truthy]
def truthy(value):
    """Whether a stored parameter value means yes: `1`, `true`, `yes`, `on` (any case)."""
    return str(value or "").strip().lower() in ("1", "true", "yes", "on")


# [@ANCHOR: hams_rpc_guard:parse_authors]
def parse_authors(value):
    """The author substrings that mark a manifest as one of ours: the comma-separated parameter, or the defaults."""
    parsed = tuple(item.strip() for item in str(value or "").split(",") if item.strip())
    return parsed or DEFAULT_MANAGED_AUTHORS


# [@ANCHOR: hams_rpc_guard:author_is_managed]
def author_is_managed(author, authors):
    """Whether the manifest `author` string contains any of the substrings `authors` (case-insensitive)."""
    lowered = str(author or "").lower()
    return any(item.lower() in lowered for item in authors)


# [@ANCHOR: hams_rpc_guard:record_verdict]
def record_verdict(call, verdict, reason, mode):
    """Count a denial (or, in report mode, a would-be denial) and log it at the first and at every power of ten.

    Only the names are logged: user class, `model.method`, reason. Never an argument, a record id, a login or an
    address. Returns the count. Allowed calls are not counted."""
    if verdict != DENY:
        return 0
    key = (mode, call.user_class, "%s.%s" % (call.model, call.method), reason)
    with _counts_lock:
        _counts[key] += 1
        count = _counts[key]
    if is_power_of_ten(count):
        _logger.warning(
            "RPC guard (%s): %s call(s) to %s.%s by a %s user, reason %s.%s",
            mode, count, call.model, call.method, call.user_class, reason,
            " The call was served: report-only mode." if mode == MODE_REPORT else "")
    return count


# [@ANCHOR: hams_rpc_guard:verdict_counts]
def verdict_counts():
    """A copy of the counters: `{(mode, user_class, "model.method", reason): count}` for this worker."""
    with _counts_lock:
        return dict(_counts)


# [@ANCHOR: hams_rpc_guard:apply_verdict]
def apply_verdict(call, verdict, reason, mode):
    """Record the verdict and, in enforce mode, refuse a denied call with `AccessError`.

    The message is the same for every refusal and names nothing but the pair, so it tells a prober no more than Odoo's
    own "cannot be called remotely" does."""
    record_verdict(call, verdict, reason, mode)
    if verdict == DENY and mode == MODE_ENFORCE:
        raise AccessError(_("The method '%s.%s' cannot be called remotely.", call.model, call.method))


