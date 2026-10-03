# -*- coding: utf-8 -*-
# Copyright © HAMS project. AGPL-3.0-or-later.
"""A fresh CSRF token for pages that may have come from Cloudflare's edge cache.

A page served from the edge cache was rendered for some earlier visitor, and ir.http drops the
unsaved session cookie from every response it marks edge-cacheable, so the CSRF token baked into
that page belongs to a session nobody holds. Before a form on such a page posts,
static/src/js/edge_cache_csrf.js asks this route for a token. This response is never cacheable
(ir.http leaves it uncached because it does not opt in, and it carries the visitor's session
cookie), so the token and the session_id cookie it is bound to arrive together.
"""
from odoo import http
from odoo.http import request


class CloudflareCsrfToken(http.Controller):

    @http.route(
        "/cloudflare/csrf_token",
        type="http",
        auth="public",
        methods=["GET"],
        readonly=True,
    )
    def csrf_token(self):
        # [@ANCHOR: COMM_cloudflare_csrf_token_route]
        # Tests [@ANCHOR: COMM_test_edge_cached_form_posts_after_token_fetch]
        # time_limit=None: the same lifetime as the token the page itself carries
        # (request.csrf_token(None) in the layout's <meta name="csrf_token">).
        return request.make_json_response(
            {"csrf_token": request.csrf_token(None)},
            headers=[("Cache-Control", "no-store")],
        )
