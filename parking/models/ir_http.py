# SPDX-License-Identifier: AGPL-3.0-or-later
"""Answers requests for parked domains from the parking.domain table (kinds are decided by tenant_sites)."""

import logging

from werkzeug.wrappers import Response

from odoo import models
from odoo.http import request
from odoo.addons.tenant_sites import utils as site_utils
from odoo.addons.tenant_sites.models.ir_http import KIND_UNKNOWN

from .. import utils

_logger = logging.getLogger(__name__)

SERVICE_USER_XMLID = "parking.user_parking_service"
INTERNAL_MARKER = "X-Parking-Response"
KIND_PARKING = "parking"
INQUIRY_PER_IP_HOUR = 5
INQUIRY_GLOBAL_HOUR = 200
MAX_BODY = 8192


class IrHttp(models.AbstractModel):
    _inherit = "ir.http"

    # ------------------------------------------------------------------ classification hooks

    # [@ANCHOR: parking:COMM_extra_kind]
    # Verified by [@ANCHOR: parking:COMM_test_extra_kind]
    @classmethod
    def _tenant_extra_kind(cls, host):
        """A hostname in the parking table is a parked domain (tenant_sites asks every module)."""
        if cls._parking_service_env()["parking.domain"]._lookup(host):
            return KIND_PARKING
        return super()._tenant_extra_kind(host)

    # [@ANCHOR: parking:COMM_public_route]
    # Verified by [@ANCHOR: parking:COMM_test_public_route]
    @classmethod
    def _tenant_public_route(cls, kind, path, method):
        """On a parked domain the for-sale form post is the one real route."""
        if kind == KIND_PARKING and path == utils.INQUIRY_PATH and method == "POST":
            return True
        return super()._tenant_public_route(kind, path, method)

    # ------------------------------------------------------------------ serving

    # [@ANCHOR: parking:COMM_serve_other]
    # Verified by [@ANCHOR: parking:COMM_test_serve_other]
    @classmethod
    def _tenant_serve_other(cls, kind):
        """A parked domain is answered from the table; an unknown hostname gets the default page
        only when `parking.unknown_host_policy` says so, else tenant_sites' plain 404."""
        if kind == KIND_PARKING:
            return cls._parking_serve()
        if kind == KIND_UNKNOWN:
            env = cls._parking_service_env()
            policy = env["ir.config_parameter"]._get_param("parking.unknown_host_policy") or "not_found"
            if policy == "default_page":
                return cls._parking_serve()
        return super()._tenant_serve_other(kind)

    # [@ANCHOR: parking:COMM_parking_service_env]
    # Verified by [@ANCHOR: parking:COMM_test_parking_service_env]
    @classmethod
    def _parking_service_env(cls):
        uid = request.env["ir.model.data"]._xmlid_to_res_id(SERVICE_USER_XMLID, raise_if_not_found=True)
        return request.env(user=uid)

    # [@ANCHOR: parking:COMM_parking_serve]
    # Verified by [@ANCHOR: parking:COMM_test_parking_serve]
    @classmethod
    def _parking_serve(cls):
        environ = request.httprequest.environ
        host = site_utils.normalize_host(site_utils.original_host(environ))
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

    # [@ANCHOR: parking:COMM_parking_page]
    # Verified by [@ANCHOR: parking:COMM_test_parking_page]
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

    # [@ANCHOR: parking:COMM_parking_response]
    # Verified by [@ANCHOR: parking:COMM_test_parking_response]
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

    # [@ANCHOR: parking:COMM_post_dispatch]
    # Verified by [@ANCHOR: parking:COMM_test_post_dispatch]
    @classmethod
    def _post_dispatch(cls, response):
        super()._post_dispatch(response)
        if response.headers.get(INTERNAL_MARKER):
            # Odoo hands every cookieless visitor a fresh session_id; a cacheable parking response
            # must never carry a Set-Cookie (Cloudflare would replay it to every later visitor).
            response.headers.setlist("Set-Cookie", [])
            response.headers.setlist(INTERNAL_MARKER, [])

    # ------------------------------------------------------------------ settings

    # [@ANCHOR: parking:COMM_parking_secret]
    # Verified by [@ANCHOR: parking:COMM_test_parking_secret]
    @classmethod
    def _parking_secret(cls, env):
        secret = env["ir.config_parameter"]._get_param("parking.form_secret")
        if not secret:
            raise RuntimeError("parking.form_secret is not set; reinstall the parking module")
        return secret
