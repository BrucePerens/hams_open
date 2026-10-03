# SPDX-License-Identifier: AGPL-3.0-or-later
"""Serves every public request of a parking instance from the parking.domain table."""

import logging

from werkzeug.exceptions import NotFound
from werkzeug.wrappers import Response

from odoo import models
from odoo.http import request

from .. import utils

_logger = logging.getLogger(__name__)

SERVICE_USER_XMLID = "parking.user_parking_service"
INTERNAL_MARKER = "X-Parking-Response"
CLOUDFLARE_PROOF_HEADER = "CF-Ray"
INQUIRY_PER_IP_HOUR = 5
INQUIRY_GLOBAL_HOUR = 200
MAX_BODY = 8192


class IrHttp(models.AbstractModel):
    _inherit = "ir.http"

    # ------------------------------------------------------------------ routing guard

    # [@ANCHOR: parking:COMM_admin_request]
    # Verified by [@ANCHOR: parking:COMM_test_admin_request]
    @classmethod
    def _parking_is_admin_request(cls):
        """True only for the operator: the socket peer is a loopback address (an SSH tunnel to the
        instance's own port) AND the request shows no sign of Cloudflare. cloudflared connects from
        loopback too, but every request through Cloudflare carries CF-Ray (the edge sets it itself),
        so no visitor reaches the backend, whatever Host or X-Forwarded-For header they send."""
        headers = request.httprequest.headers
        if headers.get(CLOUDFLARE_PROOF_HEADER) or headers.get("CF-Connecting-IP"):
            return False
        return utils.is_loopback_address(utils.original_peer(request.httprequest.environ))

    # [@ANCHOR: parking:COMM_match_guard]
    # Verified by [@ANCHOR: parking:COMM_test_match_guard]
    @classmethod
    def _match(cls, path_info):
        """On a public host nothing but the for-sale form post is a route; every other path, the
        whole backend included, is a not-found that falls through to _serve_fallback."""
        if cls._parking_is_admin_request():
            return super()._match(path_info)
        # Decided BEFORE the router runs: a wrong-method match (GET on a POST-only route) would
        # otherwise surface as 405 and tell a visitor which backend routes exist.
        if path_info == utils.INQUIRY_PATH and request.httprequest.method == "POST":
            return super()._match(path_info)
        raise NotFound()

    # ------------------------------------------------------------------ serving

    # [@ANCHOR: parking:COMM_serve_fallback]
    # Verified by [@ANCHOR: parking:COMM_test_serve_fallback]
    @classmethod
    def _serve_fallback(cls):
        if cls._parking_is_admin_request():
            return super()._serve_fallback()
        return cls._parking_serve()

    @classmethod
    def _parking_service_env(cls):
        uid = request.env["ir.model.data"]._xmlid_to_res_id(SERVICE_USER_XMLID, raise_if_not_found=True)
        return request.env(user=uid)

    @classmethod
    def _parking_serve(cls):
        environ = request.httprequest.environ
        host = utils.normalize_host(utils.original_host(environ))
        method = request.httprequest.method
        if method not in ("GET", "HEAD", "POST"):
            return cls._parking_response("method not allowed", 405, "text/plain", None)
        if not host:
            return cls._parking_response("bad host", 400, "text/plain", None)
        env = cls._parking_service_env()
        record = env["parking.domain"]._lookup(host)
        if not record:
            policy = env["ir.config_parameter"]._get_param("parking.unknown_host_policy") or "not_found"
            if policy != "default_page":
                return cls._parking_response("unknown host", 404, "text/plain", None)
            return cls._parking_page(host, None, env)
        if method == "POST":
            return cls._parking_response("method not allowed", 405, "text/plain", None)
        path = request.httprequest.path
        if path == "/robots.txt":
            return cls._parking_response(utils.robots_txt(record.noindex), 200, "text/plain", record)
        if path == "/favicon.ico":
            return cls._parking_response("", 204, "text/plain", record)
        return cls._parking_page(host, record, env)

    @classmethod
    def _parking_page(cls, host, record, env):
        behavior = record.behavior if record else "parked"
        noindex = record.noindex if record else True
        if behavior == "gone":
            return cls._parking_response(utils.render_gone(host), 410, "text/html", record)
        if behavior == "redirect":
            location = utils.redirect_location(
                record.redirect_url,
                request.httprequest.path,
                request.httprequest.query_string.decode("latin-1"),
                record.preserve_path,
            )
            response = cls._parking_response("", int(record.redirect_code), "text/html", record)
            response.headers["Location"] = location
            return response
        if behavior == "for_sale":
            secret = cls._parking_secret(env)
            sent = request.httprequest.args.get("sent") == "1"
            body = utils.render_for_sale(
                host, utils.form_token(secret, host), record.title, record.message,
                record.price_text, sent, noindex,
            )
            return cls._parking_response(body, 200, "text/html", record)
        body = utils.render_parked(
            host, record.title if record else "", record.message if record else "", noindex
        )
        return cls._parking_response(body, 200, "text/html", record)

    @classmethod
    def _parking_response(cls, body, status, content_type, record):
        """Every parking response: no cookie, a content-security-policy that allows no script, and
        a cache policy. Unknown hosts and errors are never cacheable."""
        response = Response(body, status=status, content_type=f"{content_type}; charset=utf-8")
        headers = response.headers
        headers[INTERNAL_MARKER] = "1"
        headers["X-Content-Type-Options"] = "nosniff"
        headers["Referrer-Policy"] = "no-referrer"
        headers["Content-Security-Policy"] = (
            "default-src 'none'; style-src 'unsafe-inline'; form-action 'self'; "
            "base-uri 'none'; frame-ancestors 'none'"
        )
        if record is None or status >= 400 and status != 410:
            headers["Cache-Control"] = "no-store"
        else:
            ttl = max(record.cache_ttl, 0)
            headers["Cache-Control"] = f"public, max-age={min(ttl, 300)}, s-maxage={ttl}"
        if record is None or record.noindex:
            headers["X-Robots-Tag"] = "noindex, nofollow"
        return response

    @classmethod
    def _post_dispatch(cls, response):
        super()._post_dispatch(response)
        if response.headers.get(INTERNAL_MARKER):
            # Odoo hands every cookieless visitor a fresh session_id; a cacheable parking response
            # must never carry a Set-Cookie (Cloudflare would replay it to every later visitor).
            response.headers.setlist("Set-Cookie", [])
            response.headers.setlist(INTERNAL_MARKER, [])

    # ------------------------------------------------------------------ settings

    @classmethod
    def _parking_secret(cls, env):
        secret = env["ir.config_parameter"]._get_param("parking.form_secret")
        if not secret:
            raise RuntimeError("parking.form_secret is not set; reinstall the parking module")
        return secret
