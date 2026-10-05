# -*- coding: utf-8 -*-
"""Production lockdown of Odoo's database-manager routes and of JSON error bodies.

hams1 readiness audit 2026-10-04, row 15: `/web/database/manager` and `/web/database/selector` answered 200 to
anyone (inert only because `list_db` is off and no `admin_passwd` is set), and a malformed JSON-RPC POST returned
the full Python traceback, with file paths and line numbers, in `error.data.debug`.

This has to hold for every installation of this open-source module, not just ours, so it is in code and not in a
firewall or Cloudflare rule: unless the server runs in developer mode (`--dev`), every `/web/database/*` route is
a plain 404, and the JSON-RPC and JSON-2 dispatchers never put a traceback or an unexpected exception's message
in a response. Expected errors (UserError, AccessDenied, validation, 404, expired session) keep their message,
which is written for the user; anything else becomes a generic "Odoo Server Error" and is still logged in full
by Odoo on the server. No Odoo core file is edited: the Database controller is subclassed (an overriding method
keeps its parent's route) and the two dispatchers are subclassed under the same routing type, which replaces the
core ones.
"""
import collections.abc
import http as http_status_module
import logging

from werkzeug.exceptions import HTTPException, NotFound

from odoo import http, tools
from odoo.addons.web.controllers.database import Database
from odoo.exceptions import AccessDenied, RedirectWarning, UserError
from odoo.http import Json2Dispatcher, JsonRPCDispatcher, SessionExpiredException

_logger = logging.getLogger(__name__)

GENERIC_MESSAGE = "Odoo Server Error"
EXPECTED_ERRORS = (UserError, AccessDenied, RedirectWarning, SessionExpiredException, HTTPException)


# [@ANCHOR: hams_base:developer_mode_check]
def developer_mode():
    """True when the server was started with --dev (a developer's own machine, never production)."""
    return bool(tools.config.get("dev_mode"))


# [@ANCHOR: hams_base:scrub_error_body]
def scrub_error_body(data, exc):
    """Return `data` (an `odoo.http.serialize_exception` result) made safe to send to a client.

    The traceback is always dropped. For an exception that is not one of the expected, user-facing kinds the
    class name, message, arguments and context are replaced too, since they can carry SQL, paths or secrets.
    In developer mode `data` is returned untouched."""
    if developer_mode():
        return data
    data = dict(data)
    data["debug"] = ""
    if not isinstance(exc, EXPECTED_ERRORS):
        data.update(name="odoo.exceptions.UserError", message=GENERIC_MESSAGE, arguments=[GENERIC_MESSAGE], context={})
    return data


class DatabaseManagerLockdown(Database):
    """Every `/web/database/*` route is a 404 outside developer mode.

    Each override keeps the route (`type`, `auth`, `methods`) of the method it replaces."""

    @staticmethod
    def _refuse():
        if not developer_mode():
            raise NotFound()

    def selector(self, **kw):
        self._refuse()
        return super().selector(**kw)

    def manager(self, **kw):
        self._refuse()
        return super().manager(**kw)

    def create(self, *args, **kw):
        self._refuse()
        return super().create(*args, **kw)

    def duplicate(self, *args, **kw):
        self._refuse()
        return super().duplicate(*args, **kw)

    def drop(self, *args, **kw):
        self._refuse()
        return super().drop(*args, **kw)

    def backup(self, *args, **kw):
        self._refuse()
        return super().backup(*args, **kw)

    def restore(self, *args, **kw):
        self._refuse()
        return super().restore(*args, **kw)

    def change_password(self, *args, **kw):
        self._refuse()
        return super().change_password(*args, **kw)

    def list(self, *args, **kw):
        self._refuse()
        return super().list(*args, **kw)


# Same `routing_type` as the core class: Dispatcher.__init_subclass__ registers by routing type, so these replace it.
class HamsJsonRPCDispatcher(JsonRPCDispatcher):
    routing_type = "jsonrpc"

    def handle_error(self, exc: Exception) -> collections.abc.Callable:
        error = {
            "code": 0,
            "message": GENERIC_MESSAGE,
            "data": scrub_error_body(http.serialize_exception(exc), exc),
        }
        if isinstance(exc, NotFound):
            error["code"] = 404
            error["message"] = "404: Not Found"
        elif isinstance(exc, SessionExpiredException):
            error["code"] = 100
            error["message"] = "Odoo Session Expired"
        return self._response(error=error)


class HamsJson2Dispatcher(Json2Dispatcher):
    routing_type = "json2"

    def handle_error(self, exc: Exception) -> collections.abc.Callable:
        if isinstance(exc, HTTPException) and exc.response:
            return exc.response

        headers = None
        if isinstance(exc, (UserError, SessionExpiredException)):
            status = exc.http_status
            body = http.serialize_exception(exc)
        elif isinstance(exc, HTTPException):
            status = exc.code
            body = http.serialize_exception(exc, message=exc.description, arguments=(exc.description, exc.code))
            content_type, *headers = exc.get_headers()
            assert content_type == ("Content-Type", "text/html; charset=utf-8")
        else:
            status = http_status_module.HTTPStatus.INTERNAL_SERVER_ERROR
            body = http.serialize_exception(exc)
        return self.request.make_json_response(scrub_error_body(body, exc), headers=headers, status=status)
