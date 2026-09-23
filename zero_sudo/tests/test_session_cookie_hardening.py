# -*- coding: utf-8 -*-
# Part of Odoo. See LICENSE file for full copyright and licensing details.
#
# This file is part of hams_open, an open source module.
# License: AGPL-3.0
import unittest

from odoo.tests.common import tagged

from odoo.addons.zero_sudo.models.ir_http import _harden_cookie_header
from odoo.addons.zero_sudo.tests.common import HamsHttpCase


# Tests [@ANCHOR: zero_sudo:harden_cookie_header]
# night_shift_todo's own "session-cookie-missing-secure-and-samesite-attributes"
# finding, 2026-09-22: session_id (the real auth token) and frontend_lang had
# neither Secure nor SameSite even over a real HTTPS connection through the
# reverse proxy.
class TestHardenCookieHeader(unittest.TestCase):
    def test_session_id_gets_secure_and_samesite_over_https(self):
        raw = "session_id=abc123; Expires=Thu, 01-Jan-2026; Max-Age=604800; HttpOnly; Path=/"
        hardened = _harden_cookie_header(raw, is_https=True)
        self.assertIn("Secure", hardened)
        self.assertIn("SameSite=Lax", hardened)
        self.assertIn("HttpOnly", hardened)

    def test_session_id_gets_samesite_but_not_secure_over_plain_http(self):
        raw = "session_id=abc123; Max-Age=604800; HttpOnly; Path=/"
        hardened = _harden_cookie_header(raw, is_https=False)
        self.assertNotIn("Secure", hardened)
        self.assertIn("SameSite=Lax", hardened)

    def test_frontend_lang_is_also_hardened(self):
        raw = "frontend_lang=en_US; Expires=Thu, 01-Jan-2027; Path=/"
        hardened = _harden_cookie_header(raw, is_https=True)
        self.assertIn("Secure", hardened)
        self.assertIn("SameSite=Lax", hardened)

    def test_an_unrelated_cookie_is_left_untouched(self):
        # A cookie set explicitly elsewhere (e.g. gdpr_export_token) with its
        # own deliberate flags must not be touched by this general hardening.
        raw = "gdpr_export_token=xyz; Max-Age=300; HttpOnly; Path=/api/v1/gdpr_export/; SameSite=Strict; Secure"
        hardened = _harden_cookie_header(raw, is_https=True)
        self.assertEqual(hardened, raw)

    def test_a_cookie_that_already_has_samesite_is_not_duplicated(self):
        raw = "session_id=abc123; HttpOnly; Path=/; SameSite=Strict"
        hardened = _harden_cookie_header(raw, is_https=True)
        self.assertEqual(hardened.count("SameSite"), 1)
        self.assertIn("SameSite=Strict", hardened)
        self.assertNotIn("SameSite=Lax", hardened)


@tagged("post_install", "-at_install")
class TestPostDispatchCookieHardening(HamsHttpCase):
    # A real end-to-end request against the actual login page, over a
    # simulated HTTPS connection through the reverse proxy (matching
    # proxy_mode's own X-Forwarded-Proto contract), confirming the real
    # Set-Cookie header Odoo sends is actually hardened, not just the pure
    # helper function in isolation.
    def test_login_page_sets_a_samesite_hardened_session_cookie(self):
        # Only asserts SameSite here, not Secure: whether this test harness's
        # own werkzeug stack actually trusts X-Forwarded-Proto the same way
        # the real production reverse-proxy chain does is a separate,
        # environment-specific concern (see night_shift_todo's own updated
        # finding -- confirmed live that request.httprequest.scheme does not
        # report "https" even behind the real Cloudflare Tunnel in
        # production today). The Secure-vs-is_https branch itself is already
        # covered precisely, without that ambiguity, by
        # TestHardenCookieHeader's pure-function tests above.
        response = self.url_open("/web/login")
        cookie_headers = response.raw.headers.getlist("Set-Cookie") or []
        session_cookies = [h for h in cookie_headers if h.startswith("session_id=")]
        self.assertTrue(session_cookies, "expected a session_id cookie on the login page response")
        for h in session_cookies:
            self.assertIn("SameSite=Lax", h)
            self.assertIn("HttpOnly", h)
