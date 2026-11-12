# -*- coding: utf-8 -*-
# Copyright © Bruce Perens K6BP.
# SPDX-License-Identifier: AGPL-3.0-or-later
"""`@public_rpc`: declares a model method that is meant to be called over RPC (a view button, JS, a daemon, a core override).

It lives here, in `zero_sudo`, so every addon can use it without depending on `hams_rpc_guard`. It only sets an
attribute on the function; `hams_rpc_guard` reads that attribute (`MARK`) to admit the call, and its source scanner
(`exposure_scan.py`) recognises the decorator by name, so a marked method needs no entry in `rpc_exposure.json`.
"""

MARK = "_hams_public_rpc"


# [@ANCHOR: zero_sudo:public_rpc]
def public_rpc(func=None, *, portal=False, clients=()):
    """Declare a model method callable over RPC.

    Use bare (`@public_rpc`) for a method any signed-in internal user may call, or with arguments:
    `@public_rpc(portal=True)` also admits portal and public users (strict mode), `@public_rpc(clients=("login",))`
    additionally admits those service-account logins. Put it above `@api.model`/`@api.depends`... and below nothing
    that wraps the function in a new object: it only sets an attribute on the function it is given."""
    def mark(function):
        setattr(function, MARK, {"portal": bool(portal), "clients": tuple(clients)})
        return function
    return mark(func) if callable(func) else mark
