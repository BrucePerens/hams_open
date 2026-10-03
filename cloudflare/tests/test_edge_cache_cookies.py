# Copyright © HAMS project.
# SPDX-License-Identifier: AGPL-3.0-or-later
"""Responses marked edge-cacheable never carry Set-Cookie; forms on them still post.

Real requests through Odoo's HTTP stack (HttpCase), so every _post_dispatch override, Odoo's own
session save and website.page's own page cache all run exactly as in production. Each scenario
starts from a fresh cookie jar (_fresh_visitor), because HttpCase's opener otherwise carries the
session cookie of an earlier request into the next one.
"""
import re
from unittest.mock import patch

from odoo.tests.common import Opener, tagged
from odoo.addons.zero_sudo.tests.common import HamsHttpCase
from odoo.addons.cloudflare.models.ir_http import (
    EDGE_CACHEABLE_MARKER,
    _set_cookie_name,
    _set_cookie_value,
)

CDN = "Cloudflare-CDN-Cache-Control"
NO_STORE = "no-cache, no-store"
PAGE_URL = "/cf-edge-cache-test"
# Any CSRF token Odoo baked into the page (request.csrf_token(): 40 hex digits, "o", expiry). Its
# location differs by install (an inline `csrf_token: "..."` script in stock Odoo, a
# <meta name="csrf_token"> under hams_com's content_security_policy, form inputs); all are dead.
BAKED_TOKEN_RE = re.compile(r"\b([0-9a-f]{40}o\d+)\b")


@tagged("post_install", "-at_install")
class TestEdgeCacheCookies(HamsHttpCase):

    def setUp(self):
        super().setUp()
        self.env["website.page"].create(
            {
                "url": PAGE_URL,
                "name": "Edge cache test page",
                "type": "qweb",
                "is_published": True,
                "arch": (
                    '<t name="Edge cache test page" t-name="cloudflare.edge_cache_test_page">'
                    '<t t-call="website.layout"><div id="wrap">Edge cache test content</div></t>'
                    "</t>"
                ),
            }
        )
        self.portal_user = self.env["res.users"].create(
            {
                "name": "CF Edge Tester",
                "login": "cf_edge_tester",
                "password": "cf_edge_tester_pw",
                "group_ids": [(6, 0, [self.env.ref("base.group_portal").id])],
            }
        )

    def _fresh_visitor(self):
        self.opener = Opener(self)

    def _cookie_names(self, response):
        return [_set_cookie_name(value) for value in response.raw.headers.getlist("Set-Cookie")]

    def test_set_cookie_parsing(self):
        # Tests [@ANCHOR: COMM_cloudflare_strip_redundant_cookies]
        raw = "session_id=abc=def; Expires=Sun, 03 Oct 2027 04:47:29 GMT; HttpOnly; Path=/"
        self.assertEqual(_set_cookie_name(raw), "session_id")
        self.assertEqual(_set_cookie_value(raw), "abc=def")
        self.assertEqual(_set_cookie_name("frontend_lang=en_US"), "frontend_lang")

    def test_edge_cacheable_page_has_no_set_cookie(self):
        # [@ANCHOR: COMM_test_edge_cacheable_page_has_no_set_cookie]
        # Tests [@ANCHOR: COMM_cloudflare_edge_cacheable]
        # Tests [@ANCHOR: COMM_website_page_edge_cache_opt_in]
        """A first anonymous visit to a plain website page is edge-cacheable and sets no cookie."""
        self._fresh_visitor()
        response = self.url_open(PAGE_URL)
        self.assertEqual(response.status_code, 200)
        self.assertIn("Edge cache test content", response.text)
        self.assertEqual(response.headers.get(CDN), "max-age=86400")
        self.assertNotIn("Set-Cookie", response.headers, "an edge-cacheable response must set no cookie")
        self.assertNotIn(EDGE_CACHEABLE_MARKER, response.headers, "the internal marker must not leak")
        self.assertFalse(self.opener.cookies, "the browser must come away with no cookie at all")

        # The same page, for a visitor who already holds a session or a consent choice, is not.
        for cookie in ("session_id", "website_cookies_bar"):
            self._fresh_visitor()
            response = self.url_open(PAGE_URL, cookies={cookie: "x"})
            self.assertEqual(response.status_code, 200)
            self.assertEqual(response.headers.get(CDN), NO_STORE, f"request carrying {cookie}")

        # Query parameters are not in Odoo's page-cache key, so Odoo refuses to cache: neither do we.
        self._fresh_visitor()
        response = self.url_open(PAGE_URL + "?utm_source=x")
        self.assertEqual(response.headers.get(CDN), NO_STORE)

        # A logged-in visitor never gets a cacheable response.
        self._fresh_visitor()
        self.authenticate("cf_edge_tester", "cf_edge_tester_pw")
        response = self.url_open(PAGE_URL)
        self.assertEqual(response.status_code, 200)
        self.assertEqual(response.headers.get(CDN), NO_STORE)

    def test_login_page_and_post_targets_are_never_cacheable(self):
        # Tests [@ANCHOR: COMM_cloudflare_edge_cacheable]
        # Tests [@ANCHOR: cf_nocache_routes]
        """Login keeps working exactly as before: uncached, and it still hands out the session."""
        self._fresh_visitor()
        response = self.url_open("/web/login")
        self.assertEqual(response.status_code, 200)
        self.assertEqual(response.headers.get(CDN), NO_STORE)
        self.assertIn("session_id", self._cookie_names(response), "/web/login must still set the session")

        token = self.url_open("/cloudflare/csrf_token").json()["csrf_token"]
        response = self.url_open("/website/form", data={"csrf_token": token})
        self.assertEqual(response.status_code, 200)
        self.assertEqual(response.headers.get(CDN), NO_STORE, "a POST target is never edge-cacheable")

    def test_edge_cached_form_posts_after_token_fetch(self):
        # [@ANCHOR: COMM_test_edge_cached_form_posts_after_token_fetch]
        # Tests [@ANCHOR: COMM_cloudflare_csrf_token_route]
        """The token baked into a cacheable page is dead; the client-side refresh makes it work.

        This is the exchange static/src/js/edge_cache_csrf.js performs, at the HTTP level.
        /website/form (POST) is Odoo core's own CSRF-checked route.
        """
        self._fresh_visitor()
        page = self.url_open(PAGE_URL)
        self.assertEqual(page.headers.get(CDN), "max-age=86400")
        baked = BAKED_TOKEN_RE.search(page.text)
        self.assertTrue(baked, "the layout must carry a CSRF token")

        # The page's own token is bound to a session that was never handed out.
        response = self.url_open("/website/form", data={"csrf_token": baked.group(1)})
        self.assertEqual(response.status_code, 400, "a token from an edge-cacheable page must not validate")

        self._fresh_visitor()
        self.url_open(PAGE_URL)
        fetched = self.url_open("/cloudflare/csrf_token")
        self.assertEqual(fetched.status_code, 200)
        self.assertEqual(fetched.headers.get(CDN), NO_STORE)
        self.assertIn("no-store", fetched.headers.get("Cache-Control", ""))
        self.assertIn("session_id", self._cookie_names(fetched), "the token endpoint must hand out the session")
        response = self.url_open("/website/form", data={"csrf_token": fetched.json()["csrf_token"]})
        self.assertEqual(response.status_code, 200, "a fetched token must validate")

        # Now holding a session, the visitor no longer gets edge-cacheable pages.
        self.assertEqual(self.url_open(PAGE_URL).headers.get(CDN), NO_STORE)

    def test_page_cache_never_replays_a_set_cookie(self):
        # [@ANCHOR: COMM_test_page_cache_never_replays_a_set_cookie]
        # Tests [@ANCHOR: COMM_website_page_no_inherited_set_cookie]
        """Odoo's page cache must not hand one visitor's session cookie to the next.

        When a cached page is older than website.page._CACHE_DURATION, Odoo re-renders it and
        returns the very object it stores, and the dispatcher then appends the current visitor's
        Set-Cookie to it. Visitor A (no session, but a consent cookie, so nothing strips A's new
        session cookie) refreshes the entry; visitor B, who already holds a session, is then served
        a cache hit and must not receive A's session_id.
        """
        page_model = type(self.env["website.page"])
        self._fresh_visitor()
        self.url_open(PAGE_URL)  # fill Odoo's page cache
        with patch.object(page_model, "_CACHE_DURATION", -1):
            self._fresh_visitor()
            visitor_a = self.url_open(PAGE_URL, cookies={"website_cookies_bar": '{"optional": true}'})
        self.assertEqual(visitor_a.status_code, 200)
        a_sids = [
            _set_cookie_value(value)
            for value in visitor_a.raw.headers.getlist("Set-Cookie")
            if _set_cookie_name(value) == "session_id"
        ]
        self.assertTrue(a_sids, "visitor A must have been handed a session cookie")

        self._fresh_visitor()
        # Same consent cookie as A, so B shares A's page-cache key (it includes the consent state).
        visitor_b = self.url_open(
            PAGE_URL, cookies={"session_id": "b" * 84, "website_cookies_bar": '{"optional": true}'}
        )
        self.assertEqual(visitor_b.status_code, 200)
        for value in visitor_b.raw.headers.getlist("Set-Cookie"):
            self.assertNotIn(a_sids[0], value, "visitor B was handed visitor A's session cookie")
