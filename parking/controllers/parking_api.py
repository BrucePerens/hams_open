# SPDX-License-Identifier: AGPL-3.0-or-later
"""The one POST a public visitor of a parking instance can make: a for-sale inquiry."""

import datetime
import logging

from odoo import fields, http
from odoo.http import request

from .. import utils
from ..models.ir_http import INQUIRY_GLOBAL_HOUR, INQUIRY_PER_IP_HOUR, MAX_BODY

_logger = logging.getLogger(__name__)


class ParkingController(http.Controller):
    # [@ANCHOR: parking:COMM_inquiry_controller]
    # Verified by [@ANCHOR: parking:COMM_test_inquiry_controller]
    # csrf=False: Odoo's CSRF token needs a session, and a parking response sets no cookie. The
    # form carries a stateless HMAC token bound to the host instead (utils.form_token).
    @http.route(utils.INQUIRY_PATH, type="http", auth="public", methods=["POST"], csrf=False,
                readonly=False, save_session=False)
    def inquiry(self, token="", name="", email="", message="", website="", **_ignored):
        ir_http = request.env["ir.http"]
        environ = request.httprequest.environ
        host = utils.normalize_host(utils.original_host(environ))
        env = ir_http._parking_service_env()
        record = env["parking.domain"]._lookup(host) if host else env["parking.domain"]
        secret = env["ir.config_parameter"]._get_param("parking.form_secret")
        if not record or record.behavior != "for_sale" or not secret:
            return ir_http._parking_response("not found", 404, "text/plain", None)
        if (request.httprequest.content_length or 0) > MAX_BODY:
            return ir_http._parking_response("too large", 413, "text/plain", None)
        token_ok = utils.verify_form_token(secret, host, token)
        if website or not token_ok:
            # A filled honeypot or a bad token: answer exactly like success so a bot learns nothing.
            return self._done(ir_http, record)
        email = (email or "").strip()[:200]
        message = (message or "").strip()[:4000]
        if "@" not in email or not message:
            return ir_http._parking_response("email and message are required", 400, "text/plain", None)
        ip_hash = utils.hash_ip(secret, utils.client_ip(environ, request.httprequest.headers))
        if self._rate_limited(env, ip_hash):
            return ir_http._parking_response("too many inquiries; try again later", 429, "text/plain", None)
        env["parking.inquiry"].create(
            {
                "domain_id": record.id,
                "name": (name or "").strip()[:120],
                "email": email,
                "message": message,
                "ip_hash": ip_hash,
            }
        )
        return self._done(ir_http, record)

    def _rate_limited(self, env, ip_hash):
        since = fields.Datetime.to_string(datetime.datetime.utcnow() - datetime.timedelta(hours=1))
        inquiries = env["parking.inquiry"]
        per_ip = inquiries.search_count([("ip_hash", "=", ip_hash), ("create_date", ">=", since)])
        everyone = inquiries.search_count([("create_date", ">=", since)])
        return per_ip >= INQUIRY_PER_IP_HOUR or everyone >= INQUIRY_GLOBAL_HOUR

    def _done(self, ir_http, record):
        response = ir_http._parking_response("", 303, "text/html", None)
        response.headers["Location"] = "/?sent=1"
        return response
