# -*- coding: utf-8 -*-
# Copyright © HAMS project. AGPL-3.0-or-later.
from odoo import http, models

from .ir_http import EDGE_CACHEABLE_MARKER


class WebsitePage(models.Model):
    _inherit = "website.page"

    def _get_response(self, request):
        # [@ANCHOR: COMM_website_page_edge_cache_opt_in]
        # Tests [@ANCHOR: COMM_test_edge_cacheable_page_has_no_set_cookie]
        """Offer a page to Cloudflare's edge cache exactly when Odoo's own page cache accepts it.

        website.page._allow_to_use_cache() is Odoo's own test for "this HTML is the same for every
        anonymous visitor" (a GET, no query parameters, the public user, no group-restricted page).
        This only sets the opt-in marker; ir.http._post_dispatch() makes the final decision once
        the response's cookies are known, and removes the marker before the response leaves.
        """
        cacheable = self._allow_to_use_cache(request)
        response = super()._get_response(request)
        if response and cacheable and not response.cf_page_cache_hit:
            # [@ANCHOR: COMM_website_page_no_shared_cache_entry]
            # Tests [@ANCHOR: COMM_test_page_cache_never_replays_a_set_cookie]
            # Odoo's page cache, when an entry is older than _CACHE_DURATION, re-renders the page,
            # stores that very response object in the cache AND returns it. Everything after this
            # point then mutates the cached entry: the dispatcher appends this visitor's Set-Cookie
            # (their session_id), content_security_policy writes this request's script nonce into
            # the body with set_data() (turning it into bytes, on which the next cache hit's
            # _post_process_response_from_cache() raises TypeError: a 500 for every later
            # visitor). A cache hit is a new http.Response built for this request, flagged by
            # _post_process_response_from_cache() below; a fresh render (_get_response_raw) may be
            # the cache entry itself. Hand the dispatcher a copy and leave the entry intact.
            fresh = http.Response(
                headers=response.headers.copy(),
                mimetype=response.mimetype,
                content_type=response.content_type,
                status=response.status,
                response=list(response.response),
            )
            # website.ir_http._register_website_track() reads the main object from it. Every
            # http.Response has a qcontext (Response.set_default() runs in __init__).
            fresh.qcontext = response.qcontext
            response = fresh
        if response:
            # [@ANCHOR: COMM_website_page_no_inherited_set_cookie]
            # Tests [@ANCHOR: COMM_test_page_cache_never_replays_a_set_cookie]
            # A page response gets its own cookies only later, from request.future_response, so
            # any Set-Cookie already on it here can only have been inherited from a cache entry.
            # The copy above keeps entries clean; this drops one regardless.
            response.headers.setlist("Set-Cookie", [])
        if response and cacheable:
            response.headers[EDGE_CACHEABLE_MARKER] = "1"
        return response

    def _get_response_raw(self, request):
        # Tests [@ANCHOR: COMM_test_page_cache_never_replays_a_set_cookie]
        """Flag a freshly rendered page as not a cache hit; see _get_response()."""
        response = super()._get_response_raw(request)
        if response:
            response.cf_page_cache_hit = False
        return response

    def _post_process_response_from_cache(self, request, response):
        # Tests [@ANCHOR: COMM_test_page_cache_never_replays_a_set_cookie]
        """Flag a response built from Odoo's page cache as a cache hit; see _get_response()."""
        super()._post_process_response_from_cache(request, response)
        response.cf_page_cache_hit = True
