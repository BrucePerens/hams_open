# -*- coding: utf-8 -*-
# Copyright © Bruce Perens K6BP.
# SPDX-License-Identifier: AGPL-3.0-or-later
"""Where the policy meets Odoo: one wrapper at the single choke point every RPC path goes through.

`/web/dataset/call_kw`, `/web/dataset/call_button`, XML-RPC and JSON-RPC `execute_kw` all end in
`odoo.service.model.call_kw`, and JSON-2 calls `get_public_method` itself; `call_kw` looks `get_public_method` up in
its own module namespace at call time. So replacing `odoo.service.model.get_public_method` (and the name JSON-2 imported)
guards all of them, after the core's own checks (private names, `@api.private`) have run. A call from Python code
(a cron, a daemon's own model code, a test) never reaches a request and is never judged.

XML-RPC and JSON-RPC run `execute_cr` with the HTTP request taken off the thread's stack, so `execute_cr` is wrapped too,
to mark the call as external.
"""
import contextvars
import functools
import logging

from odoo.addons.rpc.controllers import json2
from odoo.http import request
from odoo.models import BaseModel
from odoo.service import model as service_model

from . import endpoint_gate, exposure as exposure_module, policy

_logger = logging.getLogger(__name__)

_external = contextvars.ContextVar("hams_rpc_guard_external", default=False)
BROWSER_PREFIXES = ("/web/dataset/call_kw", "/web/dataset/call_button")
WRAPPED = "_hams_rpc_guard_wrapped"
ORM_NAMES = frozenset(dir(BaseModel))


# [@ANCHOR: hams_rpc_guard:call_is_rpc]
def call_is_rpc():
    """Whether the call being served arrived over RPC: an external endpoint (`execute_cr` marks it), `/json/2`, or the
    web client's `call_kw`/`call_button`. False for Python code, cron and tests with no such request."""
    if _external.get():
        return True
    try:
        path = request.httprequest.path if request else None
    except RuntimeError:  # no request on this thread
        return False
    if not path:
        return False
    return path.startswith(BROWSER_PREFIXES) or endpoint_gate.endpoint_family(path) == endpoint_gate.FAMILY_JSON2


# [@ANCHOR: hams_rpc_guard:module_is_managed]
@functools.lru_cache(maxsize=4096)
def _module_is_managed(module, authors):
    manifest = exposure_module.manifest_of(module)
    return bool(manifest) and policy.author_is_managed(manifest.get("author", ""), authors)


# [@ANCHOR: hams_rpc_guard:describe_call]
def describe_call(model, name, authors, exposure, cert=""):
    """The `policy.Call` for `name` on the recordset `model` as seen by `model.env.user`."""
    env = model.env
    user = env.user
    if user._is_public():
        user_class, login = policy.USER_PUBLIC, ""
    elif env["ir.http"]._is_service_account_cached(user.id):
        # Before the portal test: on hams.com most service accounts are portal-type users (not in the internal group).
        user_class, login = policy.USER_SERVICE, user.login
    elif not user._is_internal():
        user_class, login = policy.USER_PORTAL, ""
    else:
        user_class, login = policy.USER_INTERNAL, ""
    defining = [cls for cls in type(model).mro() if name in cls.__dict__]
    modules = [cls.__dict__.get("_module") for cls in defining]
    # Ours only if every class that defines the method is ours: an override of a stock method keeps the stock method's behaviour.
    managed = bool(defining) and all(
        module and (module in exposure.modules or _module_is_managed(module, authors)) for module in modules)
    return policy.Call(
        model=model._name,
        method=name,
        user_class=user_class,
        login=login,
        defining_models=tuple(dict.fromkeys(cls.__dict__.get("_name") or "" for cls in defining)),
        managed=managed,
        orm_standard=name in ORM_NAMES,
        marks=exposure_module.decorator_marks(type(model), name),
        cert=cert,
    )


# [@ANCHOR: hams_rpc_guard:certificate_identity]
def certificate_identity(utils):
    """`cert:<subject>` when the operator named a header (`hams_rpc_guard.client_cert_header`) in which a trusted proxy
    on this machine puts the verified client-certificate subject, and the request has it from such a proxy; else `""`.

    `utils` is a `zero_sudo.security.utils` recordset (parameters are read through it). The header is believed only
    when the socket peer is a loopback address listed in `hams_rpc_guard.trusted_proxies`. It can only narrow a
    daemon (see `policy.decide`), so a local process that forges it gains nothing."""
    name = utils._get_system_param(policy.CLIENT_CERT_HEADER_PARAM, "")
    if not name or not request:
        return ""
    trusted = endpoint_gate.parse_trusted_proxies(utils._get_system_param(endpoint_gate.TRUSTED_PROXIES_PARAM, ""))
    if not endpoint_gate.peer_is_trusted(request.httprequest.environ, trusted):
        return ""
    subject = request.httprequest.headers.get(name)
    return "cert:" + subject.strip() if subject and subject.strip() else ""


# [@ANCHOR: hams_rpc_guard:check_rpc_call]
def check_rpc_call(model, name):
    """Judge one RPC call of method `name` on `model` and refuse it (enforce mode) or count it (report mode).

    Reads the mode, `strict_stock` and `managed_authors` parameters through `zero_sudo.security.utils`; a parameter that
    cannot be read is read as the strict value (enforce, strict_stock off, default authors)."""
    utils = model.env["zero_sudo.security.utils"]
    try:
        mode = policy.normalise_mode(utils._get_system_param(policy.MODE_PARAM, policy.DEFAULT_MODE))
        strict_stock = policy.truthy(utils._get_system_param(policy.STRICT_STOCK_PARAM, ""))
        authors = policy.parse_authors(utils._get_system_param(policy.MANAGED_AUTHORS_PARAM, ""))
        cert = certificate_identity(utils)
    except Exception:  # audit-ignore-catch-all: a parameter that cannot be read must not open the guard
        _logger.exception("Could not read the RPC guard parameters; using the strict defaults.")
        mode, strict_stock, authors, cert = policy.DEFAULT_MODE, False, policy.DEFAULT_MANAGED_AUTHORS, ""
    if mode == policy.MODE_OFF:
        return
    exposure = exposure_module.load_exposure(model.env.cr, model.env.registry)
    call = describe_call(model, name, authors, exposure, cert)
    verdict, reason = policy.decide(call, exposure, strict_stock)
    policy.apply_verdict(call, verdict, reason, mode)


# [@ANCHOR: hams_rpc_guard:guard_get_public_method]
def guard_get_public_method(original):
    """The replacement for `odoo.service.model.get_public_method`: the core's own checks first, then the guard."""
    @functools.wraps(original)
    def guarded(model, name):
        method = original(model, name)
        if call_is_rpc():
            check_rpc_call(model, name)
        return method
    guarded.__dict__[WRAPPED] = True
    return guarded


# [@ANCHOR: hams_rpc_guard:guard_execute_cr]
def guard_execute_cr(original):
    """The replacement for `odoo.service.model.execute_cr`: marks everything inside it as an external RPC call."""
    @functools.wraps(original)
    def guarded(*args, **kwargs):
        token = _external.set(True)
        try:
            return original(*args, **kwargs)
        finally:
            _external.reset(token)
    guarded.__dict__[WRAPPED] = True
    return guarded


# [@ANCHOR: hams_rpc_guard:install]
def install():
    """Replace the three names once (idempotent). Called from the module's `post_load` hook, before any request."""
    if not service_model.get_public_method.__dict__.get(WRAPPED):
        service_model.get_public_method = guard_get_public_method(service_model.get_public_method)
    if not service_model.execute_cr.__dict__.get(WRAPPED):
        service_model.execute_cr = guard_execute_cr(service_model.execute_cr)
    if not json2.get_public_method.__dict__.get(WRAPPED):
        json2.get_public_method = service_model.get_public_method
