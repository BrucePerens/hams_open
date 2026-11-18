# -*- coding: utf-8 -*-
# Copyright © HAMS project. AGPL-3.0-or-later.
"""RFC 9116 /.well-known/security.txt, served on every website."""
from datetime import datetime, timedelta, timezone

from odoo import http
from odoo.http import request

SECURITY_CONTACT = "support@hams.com"
# RFC 9116 asks for an Expires under a year ahead; it is generated per request, so it is never stale.
EXPIRES_DAYS = 180


# [@ANCHOR: edge_cache_security_txt_security_txt]
class SecurityTxt(http.Controller):

    @http.route("/.well-known/security.txt", type="http", auth="public", methods=["GET"], readonly=True)
    def security_txt(self):
        # [@ANCHOR: COMM_security_txt_route]
        # Verified by [@ANCHOR: COMM_test_security_txt_served]
        expires = (datetime.now(timezone.utc) + timedelta(days=EXPIRES_DAYS)).strftime("%Y-%m-%dT%H:%M:%SZ")
        body = (
            f"Contact: mailto:{SECURITY_CONTACT}\n"
            f"Expires: {expires}\n"
            "Preferred-Languages: en\n"
            f"Canonical: {request.httprequest.url_root}.well-known/security.txt\n"
        )
        # ir.http turns this marker into the edge-cache directive (and strips it) for a cookie-free
        # anonymous 200.
        return request.make_response(
            body,
            headers=[
                ("Content-Type", "text/plain; charset=utf-8"),
                ("Cache-Control", "public, max-age=86400"),
                ("X-Cloudflare-Edge-Cacheable", "1"),
            ],
        )
