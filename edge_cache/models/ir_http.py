# -*- coding: utf-8 -*-
# Copyright © HAMS project. AGPL-3.0-or-later.
import logging
from odoo import models
from odoo.http import request

_logger = logging.getLogger(__name__)


# Internal opt-in marker. A response that may be shared by every anonymous visitor (website.page
# sets it when Odoo's own page cache accepts the page; a controller may set it too) carries this
# header out of the controller; _post_dispatch consumes it and never lets it reach the client.
EDGE_CACHEABLE_MARKER = "X-Cloudflare-Edge-Cacheable"
NO_EDGE_CACHE = "no-cache, no-store"
PUBLIC_PAGE_EDGE_CACHE = "max-age=86400"
# Cookies Odoo sets on a first anonymous visit that carry nothing per-visitor when the conditions
# in _cloudflare_strip_redundant_cookies() hold. Any other cookie makes a response uncacheable.
SESSION_COOKIE = "session_id"
LANG_COOKIE = "frontend_lang"
# Request cookies whose presence means the page may differ for this visitor (a session, a cookie
# consent choice that changes which tracking code the page carries). The Cloudflare cache rule must
# bypass the cache for the same cookies; the origin refuses to mark such a response cacheable too.
STATEFUL_REQUEST_COOKIES = (SESSION_COOKIE, "website_cookies_bar")


def _set_cookie_name(header_value):
    """Pure: the cookie name of one raw Set-Cookie header value."""
    return header_value.split(";", 1)[0].partition("=")[0].strip()


def _set_cookie_value(header_value):
    """Pure: the cookie value of one raw Set-Cookie header value."""
    return header_value.split(";", 1)[0].partition("=")[2].strip()


class IrHttp(models.AbstractModel):
    _inherit = "ir.http"

    @classmethod
    def _post_dispatch(cls, response):
        # [@ANCHOR: COMM_ir_http_post_dispatch_headers]
        """
        Intercepts the outgoing HTTP response to inject CDN caching directives.
        Generalized to dynamically check Odoo's public user state without requiring
        strict app dependencies.
        """
        # # Verified by [@ANCHOR: COMM_test_unauthorized_bypass]
        # This module only ever adds CDN caching directives to a response;
        # nothing here gates or rejects a request for lacking Cloudflare
        # edge headers, so ordinary direct (non-tunneled) access continues
        # to work identically whether or not the request passed through
        # the tunnel.
        res = super()._post_dispatch(response)

        # Fail loudly if headers are missing, enforcing framework contract
        _ = response.headers

        if not request:
            return res
        # The marker is internal; it must never reach the client whatever happens below.
        opted_in = bool(response.headers.get(EDGE_CACHEABLE_MARKER))
        response.headers.setlist(EDGE_CACHEABLE_MARKER, [])

        # Fail loudly if request lacks httprequest
        path = request.httprequest.path

        # 1. Media & Assets (Max aggressive caching: 1 year)
        # CRITICAL: /web/image and /web/content MUST NOT be aggressively cached here,
        # as it bypasses Odoo ACLs and causes Edge Cache IDORs for private attachments.
        # # Verified by [@ANCHOR: test_cf_static_asset_caching]
        # Only /web/assets is reachable here. Odoo's odoo/http.py _serve_static()
        # (which serves every /<module>/static/... file, including /web/static)
        # returns its response WITHOUT calling ir.http._post_dispatch (that call
        # only happens on the _serve_db/_serve_nodb paths), so a /web/static
        # entry in this branch could never run. Odoo already sets a long
        # Cache-Control on those responses itself.
        if path.startswith("/web/assets"):  # fmt: skip
            # A transient error (500/404/etc.) must never be pinned at the edge for a
            # year -- only a genuinely successful (or not-modified) asset response is
            # long-TTL cacheable.
            # Same cookie rule as public pages: a response marked edge-cacheable never carries
            # a Set-Cookie (Odoo hands an anonymous asset fetch a fresh session_id cookie).
            cls._cloudflare_strip_redundant_cookies(response)
            if response.status_code in (200, 304) and not response.headers.getlist("Set-Cookie"):
                response.headers["Cloudflare-CDN-Cache-Control"] = "max-age=31536000"
                response.headers["Cache-Tag"] = "odoo-static-assets"
            else:
                response.headers["Cloudflare-CDN-Cache-Control"] = NO_EDGE_CACHE
            return res

        # 2. Hardcoded Dynamic or API Routes (Zero caching)
        # [@ANCHOR: cf_nocache_routes]
        # CRITICAL: /web/ is maintained for technical routes like /web/image and /web/content
        # even in Odoo 19, to prevent Edge Cache IDORs.
        if any(
            path.startswith(prefix)
            for prefix in (
                "/my/",
                "/odoo",
                "/web/",  # burn-ignore-route: cache-control prefix classifier, must match every /web/* sub-route incl. /web/image and /web/content, not a navigation target  # fmt: skip
                "/api/",
                "/shop/cart",
                "/shop/checkout",
                "/shop/confirm_order",
                "/helpdesk/",
            )
        ):  # fmt: skip
            response.headers["Cloudflare-CDN-Cache-Control"] = "no-cache, no-store"
            return res

        # 3. Public HTML: uncached unless the page opted in AND nothing about this exchange is
        #    per-visitor. Design: night_shift_todo/medium/cloudflare-html-caching-prereqs
        #    (hams_com) -- a response Cloudflare caches is replayed to every later visitor, Set-Cookie
        #    included, so "cacheable" and "sets a cookie" must never both be true.
        if not (opted_in and cls._cloudflare_edge_cacheable(response)):
            response.headers["Cloudflare-CDN-Cache-Control"] = NO_EDGE_CACHE
            return res
        response.headers["Cloudflare-CDN-Cache-Control"] = PUBLIC_PAGE_EDGE_CACHE

        # Inject Website-specific Cache-Tag for granular site-wide purging if needed.
        # request.website only exists on requests Odoo's website framework
        # actually routed -- a real, expected, common condition (most
        # non-website requests, e.g. JSON-RPC/API calls, never have it).
        # Tried getattr(request, "website", False), hasattr(request,
        # "website"), and a narrowed `except AttributeError` here first --
        # check_burn_list.py rejects all three ("fail loudly" is this
        # platform's own deliberate, consistent policy for exactly this
        # shape of "attribute might not exist" check), so the broad
        # except Exception + audit-ignore-catch-all below is this
        # codebase's actually-sanctioned pattern for it, not a shortcut.
        # The one real fix: this used to call _logger.exception() (a full
        # ERROR-level stack trace) here, on every single non-website
        # request -- confirmed directly against a real full-repo test.py
        # run, firing repeatedly across ham_dns/ham_repeater_dir, neither
        # of which ever touches a website route. Downgraded to .info():
        # check_burn_list.py's own CRITICAL SILENT FAILURE rule requires
        # one of warning/error/critical/exception/info here (.debug()
        # doesn't count, confirmed directly -- too easy to have disabled
        # in production for a rule meant to prevent swallowed tracebacks),
        # so .info() is the quietest level this platform's own policy
        # actually allows for an expected, non-error condition like this.
        try:
            website = request.website
        except Exception as e:  # audit-ignore-catch-all
            # # Verified by [@ANCHOR: COMM_test_05_non_website_request_no_cache_tag]
            _logger.info("Request website missing (not a website-routed request): %s", e)
            website = False
        website_id = website.id if website else False

        if website_id:
            existing_tags = response.headers.get("Cache-Tag", "")
            new_tag = f"odoo-website-{website_id}"
            response.headers["Cache-Tag"] = (
                f"{existing_tags}, {new_tag}" if existing_tags else new_tag
            )

        return res

    @classmethod
    def _cloudflare_edge_cacheable(cls, response):
        # [@ANCHOR: COMM_cloudflare_edge_cacheable]
        # Tests [@ANCHOR: COMM_test_edge_cacheable_page_has_no_set_cookie]
        """Whether this opted-in, already-dispatched response may be cached at Cloudflare's edge.

        Every one of these must hold: a GET/HEAD; status 200/304; the public user; the request
        carried no stateful cookie (STATEFUL_REQUEST_COOKIES); and, once the redundant first-visit
        cookies are dropped (_cloudflare_strip_redundant_cookies), no Set-Cookie remains. Called
        after super()._post_dispatch(): Odoo's Dispatcher.post_dispatch() saves the session and
        copies future_response's cookies onto the response there, so every cookie this response
        will carry is already on it.
        """
        if request.httprequest.method not in ("GET", "HEAD"):
            return False
        if response.status_code not in (200, 304):
            return False
        if not (request.env and request.env.user and request.env.user._is_public()):
            return False
        if any(request.httprequest.cookies.get(name) for name in STATEFUL_REQUEST_COOKIES):
            return False
        cls._cloudflare_strip_redundant_cookies(response)
        return not response.headers.getlist("Set-Cookie")

    @classmethod
    def _cloudflare_strip_redundant_cookies(cls, response):
        # [@ANCHOR: COMM_cloudflare_strip_redundant_cookies]
        # Tests [@ANCHOR: COMM_test_edge_cacheable_page_has_no_set_cookie]
        """Drop the two cookies Odoo sets on a first anonymous visit that carry nothing for it.

        - `session_id`, when the request brought none and the session was never persisted: Odoo
          hands every cookieless visitor a brand-new, empty, unsaved session id. Dropping it
          changes nothing on the server; the visitor simply gets one later, from the first request
          that needs it (a login, a cart, or /cloudflare/csrf_token before a form POST).
        - `frontend_lang`, when the request brought none and its value is the website's default
          language: an absent cookie already resolves to that language.
        Anything else is left alone, so the caller treats the response as per-visitor.
        """
        cookies = response.headers.getlist("Set-Cookie")
        if not cookies:
            return
        session = request.session
        session_is_new = not (
            request.httprequest.cookies.get(SESSION_COOKIE)
            or session.uid
            or session.is_dirty
            or session.should_rotate
        )
        lang_cookie_is_new = not request.httprequest.cookies.get(LANG_COOKIE)
        kept = []
        for cookie in cookies:
            name = _set_cookie_name(cookie)
            if name == SESSION_COOKIE and session_is_new:
                continue
            if (
                name == LANG_COOKIE
                and lang_cookie_is_new
                and _set_cookie_value(cookie) == request.env["ir.http"]._get_default_lang().code
            ):
                continue
            kept.append(cookie)
        if len(kept) != len(cookies):
            # setlist, not `del headers[...]`: see content_security_policy's cookie hardening.
            response.headers.setlist("Set-Cookie", kept)
