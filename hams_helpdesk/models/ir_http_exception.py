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


def _missing_required_params(endpoint):
    """Names of the parameters the page handler requires that the request does not carry; [] for a
    JSON-RPC route, a handler Odoo gives no function to read, or when nothing is missing."""
    if endpoint.routing.get("type") != "http":
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


class IrHttpExceptionReport(models.AbstractModel):
    _inherit = "ir.http"

    # [@ANCHOR: hams_helpdesk:exc_ir_http_pre_dispatch]
    @classmethod
    def _pre_dispatch(cls, rule, args):
        request.httprequest.environ[ROUTE_RULE_KEY] = rule.rule
        return super()._pre_dispatch(rule, args)

    # [@ANCHOR: hams_helpdesk:missing_required_params_is_400]
    @classmethod
    def _dispatch(cls, endpoint):
        """A page route called without a parameter its handler cannot do without is a 400, not a 500.

        Stock handlers declare such parameters as required positionals (``def scoped_app(self, app_id, ...)``,
        ``def payment_confirm(self, tx_id, access_token, ...)``). Odoo calls the handler with whatever the
        request carries, so a bare GET from a scanner or a stale link raised TypeError and was a 500 and a
        server-exception ticket (QA-014, 2026-10-13: /payment/confirmation, /profile/validate_email,
        /scoped_app, /scoped_app_icon_png, /shop/pricelist; /forum/get_tags earlier, ticket #67). This is the
        client's mistake: the reply is a 400 naming the missing parameter names (never a value)."""
        missing = _missing_required_params(endpoint)
        if missing:
            raise werkzeug.exceptions.BadRequest("Missing required parameter: " + ", ".join(missing))
        return super()._dispatch(endpoint)

    # [@ANCHOR: hams_helpdesk:exc_ir_http_handle_error]
    @classmethod
    def _handle_error(cls, exception):
        report_request_exception(exception)
        return super()._handle_error(exception)
