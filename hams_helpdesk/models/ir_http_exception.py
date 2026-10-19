# Copyright © Bruce Perens K6BP.
# SPDX-License-Identifier: AGPL-3.0-or-later
"""ir.http hooks for automatic server-exception tickets (see exception_fingerprint.py).

``_handle_error`` is the one place every exception from a page controller, a JSON-RPC call or an ORM
call from the web client passes through (Odoo's ``_update_served_exception`` calls it once per failed
request). The report is made before the normal error response is built, on its own cursor, so a
broken report cannot change the response.
"""

import inspect
import logging
import traceback

import werkzeug.exceptions

from odoo import models
from odoo.http import request

from .exception_fingerprint import report_exception

_logger = logging.getLogger("hams_helpdesk.exception_reporter")

# Where _pre_dispatch leaves werkzeug's pattern for the matched route ("/community/<string:slug>").
# The raw path of a profile page contains a callsign, the pattern never does.
ROUTE_RULE_KEY = "hams_helpdesk.exception_route_rule"


# [@ANCHOR: hams_helpdesk:exc_report_request_exception]
def report_request_exception(exception):
    """Reports ``exception`` using only data that needs no SQL: the failed request's cursor may be in
    an aborted transaction. Never raises."""
    try:
        httprequest = request.httprequest
        routing_type = request.dispatcher.routing_type
        uid = request.session.uid
        report_exception(
            request.env.registry,
            exception,
            "http" if routing_type == "http" else "rpc",
            uid=uid,
            logged_in=bool(uid),
            path=httprequest.path,
            rule=httprequest.environ.get(ROUTE_RULE_KEY),
            user_agent=httprequest.headers.get("User-Agent", ""),
        )
    except Exception as error:  # audit-ignore-catch-all: Tested by [@ANCHOR: hams_helpdesk:exc_report_request_exception]
        _logger.error("Could not gather request details for an exception ticket: %r", error)


# What int() and float() say when handed text that is not a number.
_NOT_A_NUMBER_MESSAGES = ("invalid literal for int()", "could not convert string to float", "cannot convert float")

# Odoo's own date-string parsers (odoo/tools/date_utils.py). The ORM's domain optimizer calls them on any date
# value in a search domain, so a ValueError raised inside one means the request carried a date it could not read.
_DATE_PARSER_FUNCTIONS = frozenset({"parse_date", "parse_iso_date"})


# [@ANCHOR: hams_helpdesk:error_is_unreadable_date]
def _error_is_unreadable_date(error):
    """True when ``error`` was raised inside Odoo's date parser (``parse_date`` or ``parse_iso_date``).

    Decided by where the error was raised, not by its message: the parser's messages vary ("Invalid term 'x' in
    expression date", "Invalid isoformat string", "month must be in 1..12", "day is out of range for month")."""
    return any(
        frame.f_code.co_name in _DATE_PARSER_FUNCTIONS and frame.f_code.co_filename.endswith("date_utils.py")
        for frame, _line in traceback.walk_tb(error.__traceback__)
    )


# A route's own type name: "http" for a page, "jsonrpc" (older spelling "json") for a JSON-RPC call.
_CALLED_WITH_REQUEST_PARAMS = ("http", "json", "jsonrpc")


def _contains_nul(value):
    """True when a string anywhere inside `value` (a parameter dict, a list, a nested JSON body) holds a NUL
    byte. PostgreSQL cannot store or compare one, so a handler that reached the database with it raised
    ValueError and answered 500 (QA pass 2: /member/<callsign>, /repeaters?search=, /<slug>, ...)."""
    stack = [value]
    while stack:
        item = stack.pop()
        if isinstance(item, str):
            if "\x00" in item:
                return True
        elif isinstance(item, bytes):
            if b"\x00" in item:
                return True
        elif isinstance(item, dict):
            stack.extend(item.keys())
            stack.extend(item.values())
        elif isinstance(item, (list, tuple, set, frozenset)):
            stack.extend(item)
    return False


def _missing_required_params(endpoint):
    """Names of the parameters the handler requires that the request does not carry; [] for a handler
    Odoo gives no function to read, a route of another type, or when nothing is missing."""
    if endpoint.routing.get("type") not in _CALLED_WITH_REQUEST_PARAMS:
        return []
    handler = getattr(endpoint, "original_endpoint", None)
    if handler is None:
        return []
    try:
        parameters = list(inspect.signature(handler).parameters.values())[1:]  # [0] is self
    except (TypeError, ValueError):
        return []
    sent = request.params
    return [
        p.name
        for p in parameters
        if p.default is inspect.Parameter.empty
        and p.kind in (inspect.Parameter.POSITIONAL_OR_KEYWORD, inspect.Parameter.KEYWORD_ONLY)
        and p.name not in sent
    ]


# [@ANCHOR: hams_helpdesk:request_has_nul_byte]
# Verified by [@ANCHOR: hams_helpdesk:nul_byte_is_400]
def _request_has_nul_byte():
    """True when the path or any text parameter of this request holds a NUL byte (%00), at any depth of a JSON body.
    PostgreSQL refuses such a string, so every route that searched with it answered 500 and filed a
    server-exception ticket (QA pass 2, 2026-10-14: /%00, /member/%00, /repeaters?search=%00, /event?search=%00,
    /forum/help-1?search=%00 ...)."""
    return "\x00" in request.httprequest.path or _contains_nul(request.params)


class IrHttpExceptionReport(models.AbstractModel):
    _inherit = "ir.http"

    # [@ANCHOR: hams_helpdesk:exc_ir_http_pre_dispatch]
    @classmethod
    def _pre_dispatch(cls, rule, args):
        request.httprequest.environ[ROUTE_RULE_KEY] = rule.rule
        return super()._pre_dispatch(rule, args)

    # [@ANCHOR: hams_helpdesk:nul_byte_in_path]
    @classmethod
    def _match(cls, path_info):
        """A NUL byte in the path is a 400 before any converter looks anything up (the member-site converters
        search the database while the URL is matched, and the fall-back page lookup that follows a 404 does too,
        so a NUL raised ValueError there, a 500). It is a 400, not a 404, so that fall-back is not tried."""
        if "\x00" in (path_info or ""):
            raise werkzeug.exceptions.BadRequest("The address holds an invalid character.")
        return super()._match(path_info)

    # [@ANCHOR: hams_helpdesk:missing_required_params_is_400]
    @classmethod
    def _dispatch(cls, endpoint):
        """A NUL byte in a parameter, or a route called without a parameter its handler cannot do without, is
        a 400, not a 500.

        A NUL anywhere in the parameters (query, form or JSON body) is refused first (see _request_has_nul_byte).
        The same 400 now covers JSON-RPC routes: a call that left out a required argument raised TypeError
        inside the handler, a server-exception ticket per call (QA pass 2: 18 member routes under
        /repeaters, /event, /api/v1/ham_shack, /api/v1/training and /api/v1/auxcomm).

        A page route called without a parameter its handler cannot do without is a 400, not a 500.

        Stock handlers declare such parameters as required positionals (``def scoped_app(self, app_id, ...)``,
        ``def payment_confirm(self, tx_id, access_token, ...)``). Odoo calls the handler with whatever the
        request carries, so a bare GET from a scanner or a stale link raised TypeError and was a 500 and a
        server-exception ticket (QA-014, 2026-10-13: /payment/confirmation, /profile/validate_email,
        /scoped_app, /scoped_app_icon_png, /shop/pricelist; /forum/get_tags earlier, ticket #67). This is the
        client's mistake: the reply is a 400 naming the missing parameter names (never a value)."""
        if _request_has_nul_byte():
            raise werkzeug.exceptions.BadRequest("The request contains a NUL byte.")
        missing = _missing_required_params(endpoint)
        if missing:
            raise werkzeug.exceptions.BadRequest("Missing required parameter: " + ", ".join(missing))
        try:
            return super()._dispatch(endpoint)
        except ValueError as error:
            # A handler that turns a request parameter into a number with int() or float() and gets text raises
            # ValueError("invalid literal for int() ..."): the sender's mistake, not a server fault (QA pass 2:
            # /club/apply/submit?club_id=abc, /my/identity/street-quiz/submit?quiz_id=abc and the like, a 500 and
            # a ticket each). Answered 400; logged at warning so a handler that really has this bug stays visible.
            if str(error).startswith(_NOT_A_NUMBER_MESSAGES):
                _logger.warning(
                    "Answered 400 for a non-numeric parameter on %s: %s",
                    request.httprequest.environ.get(ROUTE_RULE_KEY) or request.httprequest.path,
                    error,
                )
                raise werkzeug.exceptions.BadRequest("A request parameter is not a number.") from error
            # [@ANCHOR: hams_helpdesk:bad_date_is_400]
            # Text where a date belongs: stock /blog passes ?date_begin= and ?date_end= straight into a search
            # domain, and the ORM raises ValueError while optimizing it. A scanner's /blog and /blog/<blog> requests
            # with date_begin=ijyM were 45 x 500 and server-exception tickets 72 and 73 (2026-10-16 00:34-00:35 UTC).
            # The sender's mistake: 400, logged at warning so a handler that builds a bad date itself stays visible.
            if _error_is_unreadable_date(error):
                _logger.warning(
                    "Answered 400 for an unreadable date parameter on %s: %s",
                    request.httprequest.environ.get(ROUTE_RULE_KEY) or request.httprequest.path,
                    error,
                )
                raise werkzeug.exceptions.BadRequest("A request parameter is not a date.") from error
            raise

    # [@ANCHOR: hams_helpdesk:exc_ir_http_handle_error]
    @classmethod
    def _handle_error(cls, exception):
        report_request_exception(exception)
        return super()._handle_error(exception)
