# -*- coding: utf-8 -*-
# Copyright © Bruce Perens K6BP.
# SPDX-License-Identifier: AGPL-3.0-or-later
"""The registry of which model methods may be called over RPC, by whom.

Odoo core already refuses a method whose name starts with `_` and one marked `@api.private`. Everything else public is
callable over `/web/dataset/call_kw` by any signed-in user, which is far more than a web page needs. This module lets
an addon say exactly what it exposes:

* the decorator `@public_rpc` on a method (it lives in `zero_sudo`: `from odoo.addons.zero_sudo import public_rpc`), or
* a file `rpc_exposure.json` in the addon's root directory (format in the README).

The files of every installed addon are merged into one `Exposure` per registry. Nothing here decides anything: the
decision is `policy.decide`.
"""
import json
import logging
import os
import threading
import types

from odoo.modules.module import Manifest, get_module_path

from odoo.addons.zero_sudo.rpc_public import MARK

_logger = logging.getLogger(__name__)

EXPOSURE_FILE = "rpc_exposure.json"
SECTIONS = ("internal", "portal", "stock")

CACHE_ATTRIBUTE = "_hams_rpc_guard_exposure"  # kept on the Registry object: a new registry (module change) starts empty
_cache_lock = threading.Lock()


# [@ANCHOR: hams_rpc_guard:decorator_marks]
def decorator_marks(model_class, name):
    """The `@public_rpc` marks on `name` in every class of `model_class`'s MRO that defines it, as a list of dicts."""
    marks = []
    for cls in model_class.mro():
        attribute = cls.__dict__.get(name)
        if isinstance(attribute, types.FunctionType) and attribute.__dict__.get(MARK):
            marks.append(attribute.__dict__[MARK])
    return marks


# [@ANCHOR: hams_rpc_guard_exposure_exposure]
class Exposure:
    """Everything the installed addons declared. Sets of `(model, method)` pairs; `clients` maps a login to its set."""

    # [@ANCHOR: hams_rpc_guard:exposure_init]
    def __init__(self):
        self.internal = set()
        self.portal = set()
        self.stock = set()
        self.clients = {}
        self.sources = {}
        self.modules = set()

    # [@ANCHOR: hams_rpc_guard:exposure_merge]
    def merge(self, document, origin):
        """Add one parsed `rpc_exposure.json` document. Unknown keys are ignored with a warning."""
        for key in document:
            if key not in SECTIONS + ("clients", "comment"):
                _logger.warning("%s: unknown section %r in %s", origin, key, EXPOSURE_FILE)
        for section in SECTIONS:
            getattr(self, section).update(_pairs(document.get(section)))
        clients = document.get("clients") or {}
        if not isinstance(clients, dict):  # a list here used to raise out of load_exposure and fail every RPC call
            _logger.warning("%s: 'clients' in %s is not a mapping of login to allowed methods; ignored", origin, EXPOSURE_FILE)
            clients = {}
        for login, entry in clients.items():
            allow = entry.get("allow") if isinstance(entry, dict) and "allow" in entry else entry
            self.clients.setdefault(login, set()).update(_pairs(allow))
            if isinstance(entry, dict) and isinstance(entry.get("sources"), list):
                self.sources.setdefault(login, []).extend(entry["sources"])


# [@ANCHOR: hams_rpc_guard:pairs]
def _pairs(section):
    """The `(model, method)` pairs of a `{"model.name": ["method", ...]}` mapping; anything malformed is skipped."""
    found = set()
    if isinstance(section, dict):
        for model, methods in section.items():
            if isinstance(methods, (list, tuple)):
                found.update((model, method) for method in methods if isinstance(method, str))
    return found


# [@ANCHOR: hams_rpc_guard:load_exposure]
def load_exposure(cr, registry):
    """The merged `Exposure` of the registry's installed addons, read once per registry and then cached.

    `cr` is any cursor of the database (it is used once, to list the installed addons). A missing or unreadable file
    is logged and skipped: the guard then simply knows less, and a method it does not know is denied in enforce mode,
    never allowed by accident."""
    with _cache_lock:
        cached = registry.__dict__.get(CACHE_ATTRIBUTE)
        if cached is not None:
            return cached
    cr.execute("SELECT name FROM ir_module_module WHERE state = 'installed' ORDER BY name")
    exposure = Exposure()
    for (name,) in cr.fetchall():
        path = get_module_path(name, display_warning=False)
        document_path = os.path.join(path, EXPOSURE_FILE) if path else None
        if not document_path or not os.path.isfile(document_path):
            continue
        try:
            with open(document_path, encoding="utf-8") as handle:
                document = json.load(handle)
        except (OSError, ValueError):
            _logger.exception("Cannot read %s of %s; its declarations are ignored.", EXPOSURE_FILE, name)
            continue
        if isinstance(document, dict):
            exposure.merge(document, name)
            exposure.modules.add(name)
    with _cache_lock:
        registry.__dict__[CACHE_ATTRIBUTE] = exposure
    return exposure


# [@ANCHOR: hams_rpc_guard:manifest_of]
def manifest_of(module):
    """The manifest of the addon `module` (a mapping with `author` and so on), or None when it has none."""
    return Manifest.for_addon(module, display_warning=False)
