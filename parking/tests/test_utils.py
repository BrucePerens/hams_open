# SPDX-License-Identifier: AGPL-3.0-or-later
from odoo.addons.zero_sudo.tests.common import HamsTransactionCase
from odoo.tests import tagged

from .. import utils


@tagged("post_install", "-at_install", "parking")
class TestParkingUtils(HamsTransactionCase):
    # [@ANCHOR: parking:COMM_test_validate_redirect_url]
    def test_validate_redirect_url(self):
        # Tests [@ANCHOR: parking:COMM_validate_redirect_url]
        self.assertEqual(utils.validate_redirect_url("https://target.example/path?q=1", "own.example"), "")
        for bad in ("javascript:alert(1)", "//evil.example", "ftp://x.example", "https://user:pw@x.example",
                    "https://own.example/x", "https://x.example\\@evil.example", "https://", "",
                    "https://x.example/a b", "https://[::1]/", "https://10.1.2.3/", "https://x.example:99999/"):
            self.assertNotEqual(utils.validate_redirect_url(bad, "own.example"), "", bad)

    # [@ANCHOR: parking:COMM_test_redirect_location]
    def test_redirect_location_never_changes_host(self):
        # Tests [@ANCHOR: parking:COMM_redirect_location]
        target = "https://target.example/base/"
        self.assertEqual(utils.redirect_location(target, "/x"), target)
        loc = utils.redirect_location(target, "//evil.example/x", "a=1", True)
        self.assertEqual(loc, "https://target.example/base/evil.example/x?a=1")
        loc = utils.redirect_location(target, "/\\evil.example", "", True)
        self.assertTrue(loc.startswith("https://target.example/base/"), loc)
        self.assertNotIn("\\", loc)
        loc = utils.redirect_location("https://t.example/?z=1", "/p", "a=1", True)
        self.assertEqual(loc, "https://t.example/p?z=1&a=1")
        loc = utils.redirect_location(target, "/a\r\nSet-Cookie: x=1", "", True)
        self.assertNotIn("\r", loc)
        self.assertNotIn("\n", loc)

    # [@ANCHOR: parking:COMM_test_form_token]
    # [@ANCHOR: parking:COMM_test_verify_form_token]
    def test_form_token(self):
        # Tests [@ANCHOR: parking:COMM_verify_form_token]
        # Tests [@ANCHOR: parking:COMM_form_token]
        token = utils.form_token("s3cret", "a.example", now=1000)
        self.assertTrue(utils.verify_form_token("s3cret", "a.example", token, now=1010))
        self.assertFalse(utils.verify_form_token("s3cret", "a.example", token, now=1001))
        self.assertFalse(utils.verify_form_token("s3cret", "a.example", token, now=1000 + 86401))
        self.assertFalse(utils.verify_form_token("s3cret", "b.example", token, now=1010))
        self.assertFalse(utils.verify_form_token("other", "a.example", token, now=1010))
        for junk in ("", "x", "1000.", "abc.def", None):
            self.assertFalse(utils.verify_form_token("s3cret", "a.example", junk, now=1010))

    # [@ANCHOR: parking:COMM_test_render_pages]
    # [@ANCHOR: parking:COMM_test_page]
    # [@ANCHOR: parking:COMM_test_render_for_sale]
    # [@ANCHOR: parking:COMM_test_render_gone]
    # [@ANCHOR: parking:COMM_test_robots_txt]
    def test_pages_escape_everything_they_show(self):
        # Tests [@ANCHOR: parking:COMM_page]
        # Tests [@ANCHOR: parking:COMM_render_for_sale]
        # Tests [@ANCHOR: parking:COMM_render_gone]
        # Tests [@ANCHOR: parking:COMM_robots_txt]
        # Tests [@ANCHOR: parking:COMM_render_pages]
        body = utils.render_parked("a.example", "<script>alert(1)</script>", '"><img src=x onerror=1>')
        self.assertNotIn("<script>alert", body)
        self.assertNotIn("<img", body)
        self.assertIn('content="noindex,nofollow"', body)
        self.assertNotIn("noindex", utils.render_parked("a.example", noindex=False))
        sale = utils.render_for_sale("a.example", 'tok"en', "t", "m", "<b>$1</b>")
        self.assertNotIn("<b>$1</b>", sale)
        self.assertIn(utils.INQUIRY_PATH, sale)
        self.assertLess(len(utils.render_parked("a.example")), 3000)
        gone = utils.render_gone("a.example")
        self.assertIn("410", gone)
        self.assertIn("noindex", gone)
        self.assertEqual(utils.robots_txt(True), "User-agent: *\nDisallow: /\n")
        self.assertEqual(utils.robots_txt(False), "User-agent: *\nAllow: /\n")
        self.assertIn("Thank you", utils.render_for_sale("a.example", "t", sent=True))
        self.assertNotIn("<form", utils.render_for_sale("a.example", "t", sent=True))

    # [@ANCHOR: parking:COMM_test_client_ip]
    # [@ANCHOR: parking:COMM_test_hash_ip]
    def test_client_ip_trusts_cloudflare_headers_only_with_proof(self):
        # Tests [@ANCHOR: parking:COMM_client_ip]
        # Tests [@ANCHOR: parking:COMM_hash_ip]
        environ = {"REMOTE_ADDR": "::1"}
        self.assertEqual(utils.client_ip(environ, {"CF-Connecting-IP": "9.9.9.9"}), "::1")
        both = {"CF-Connecting-IP": "9.9.9.9", "CF-Ray": "abc"}
        self.assertEqual(utils.client_ip(environ, both), "9.9.9.9")
        first = utils.hash_ip("secret", "9.9.9.9")
        self.assertEqual(first, utils.hash_ip("secret", "9.9.9.9"))
        self.assertNotEqual(first, utils.hash_ip("other", "9.9.9.9"))
        self.assertNotEqual(first, utils.hash_ip("secret", "9.9.9.8"))
        self.assertNotIn("9.9.9.9", first)
