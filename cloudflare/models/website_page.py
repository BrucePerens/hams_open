# -*- coding: utf-8 -*-
# Copyright © HAMS project. AGPL-3.0-or-later.
from odoo import models

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
        response = super()._get_response(request)
        if response:
            # [@ANCHOR: COMM_website_page_no_inherited_set_cookie]
            # Tests [@ANCHOR: COMM_test_page_cache_never_replays_a_set_cookie]
            # Odoo's page cache, when an entry is older than _CACHE_DURATION, re-renders the page,
            # stores that very response object in the cache AND returns it, so the dispatcher then
            # appends this visitor's Set-Cookie (their session_id) onto the cached object; later
            # cache hits copy its headers and would replay that cookie to other visitors. A page
            # response gets its own cookies only later, from request.future_response, so any
            # Set-Cookie already on it here was inherited from the cache: drop it.
            response.headers.setlist("Set-Cookie", [])
        if response and self._allow_to_use_cache(request):
            response.headers[EDGE_CACHEABLE_MARKER] = "1"
        return response
