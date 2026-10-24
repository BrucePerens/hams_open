# Copyright © Bruce Perens K6BP.
# SPDX-License-Identifier: AGPL-3.0-or-later
"""The beacon that turns a JavaScript error in a visitor's browser into a helpdesk exception ticket.

``POST /hams_helpdesk/browser_exception`` takes a small JSON body from
``static/src/js/browser_error_reporter.js`` and answers with a status only (no body):

* 204 recorded, held back by the once-a-minute gate or a ticket cap, or ignored as noise; a sender
  cannot tell these apart, so the endpoint tells a prober nothing;
* 400 the body is not a valid report, 403 it came from another site, 413 it is too large, 415 it is not
  JSON, 422 it is not a report of this site's own scripts (a browser extension, another site, a stale
  page), 429 the visitor is over the rate limit, 503 the limiter could not count.

CSRF: ``csrf=False`` because the endpoint does nothing on behalf of a session. The page sends the session
cookie with the report (the reporter uses ``credentials: "same-origin"``) and the server reads only the user id
from it, to LINK the signed-in member's account to the ticket so the site team can write to them; a logged-out
visitor has none and stays anonymous. The only effect is an append-only write by the exception reporter service
account, bounded by the checks below. A cross-site request cannot carry the member's cookie into a report from a
browser (``Sec-Fetch-Site`` and ``Origin`` are checked, and a cross-site JSON POST needs a CORS preflight this
endpoint never answers), so a report cannot be attributed to a member by another site.

A per-response nonce was considered and rejected: Odoo 19 has no CSP nonce support, and website pages are
cached by Odoo and Cloudflare, so a nonce would be shared by every visitor and prove nothing. What is
checked instead is cache-compatible: the same-origin signals above, a report that names the *current*
asset bundle version (it changes with every release, like a nonce that rotates per build), a top stack
frame in one of this site's own scripts, and the size, schema, rate-limit and fingerprint defences.
"""

import logging

from odoo import http
from odoo.http import request

from ..models import browser_exception_schema as schema

_logger = logging.getLogger("hams_helpdesk.exception_reporter")

ROUTE = "/hams_helpdesk/browser_exception"


def _answer(status):
    """A body-less answer that no cache keeps."""
    return http.Response("", status=status, headers=[("Cache-Control", "no-store")])


class BrowserExceptionApi(http.Controller):
    # [@ANCHOR: hams_helpdesk:browser_same_origin]
    @staticmethod
    def _is_same_origin(httprequest):
        """False when the browser says this request is cross-site.

        ``Sec-Fetch-Site`` must be ``same-origin`` when present (``none`` is a user-typed address and is
        refused too: a beacon is never navigated to). ``Origin`` must name this request's own host when
        present. When neither header is there the request is not from a modern browser's script (curl, a
        monitoring probe, an old browser); it is accepted, as the other public beacon endpoints do, and
        left to the other defences, none of which depend on the sender being honest."""
        site = httprequest.headers.get("Sec-Fetch-Site")
        if site is not None and site != "same-origin":
            return False
        origin = httprequest.headers.get("Origin")
        if origin is not None and origin.split("://", 1)[-1].lower() != httprequest.host.lower():
            return False
        return True

    # [@ANCHOR: hams_helpdesk:browser_exception_endpoint]
    @http.route(ROUTE, type="http", auth="public", methods=["POST"], csrf=False)
    def report(self, **_kwargs):
        httprequest = request.httprequest
        if not self._is_same_origin(httprequest):
            return _answer(403)
        if (httprequest.mimetype or "") != "application/json":
            return _answer(415)
        length = httprequest.content_length
        if length is None:
            return _answer(400)
        if length > schema.MAX_BODY_BYTES:
            return _answer(413)
        model = request.env["hams_helpdesk.browser.exception"]
        client = request.env["zero_sudo.security.utils"]._get_trusted_client_ip()
        if model._rate_limited(client or "unknown"):
            return _answer(429)
        raw = httprequest.get_data()[: schema.MAX_BODY_BYTES + 1]
        try:
            data = schema.parse_body(raw)
            report = schema.build_report(
                data,
                own_host=httprequest.host,
                bot_agent=schema.is_bot_agent(httprequest.headers.get("User-Agent", "")),
            )
        except schema.ReportRejected as rejected:
            _logger.debug("Browser exception report refused: %s", rejected.reason)
            return _answer(rejected.status)
        try:
            if not model._release_is_current(report.bundle, report.version):
                return _answer(422)
            model._record(report, uid=request.session.uid)
        except Exception as error:  # audit-ignore-catch-all: Tested by [@ANCHOR: hams_helpdesk:browser_exception_endpoint]
            # The beacon must never become an error report of its own, or a broken reporter would file itself.
            _logger.error("Could not record a browser exception report: %r", error, exc_info=True)
            return _answer(503)
        return _answer(204)
