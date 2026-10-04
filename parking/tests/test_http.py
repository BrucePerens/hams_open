# SPDX-License-Identifier: AGPL-3.0-or-later
import re

from odoo.addons.website.tools import MockRequest
from odoo.addons.zero_sudo.tests.common import HamsHttpCase
from odoo.tests import tagged

from .. import utils

# Every request Cloudflare forwards carries CF-Ray; the tests send it too, so they walk the same
# path as production. Requests without it come from the operator's own loopback connection.
VIA_CLOUDFLARE = {"CF-Ray": "test-SJC"}


def host(name, **extra):
    headers = {"Host": name}
    headers.update(extra)
    return headers


@tagged("post_install", "-at_install", "parking")
class TestParkingHttp(HamsHttpCase):
    @classmethod
    def setUpClass(cls):
        super().setUpClass()
        cls.env["tenant.site.own.host"].create([{"name": "main.parking-test.example"}, {"name": "*.main.parking-test.example"}])
        Domain = cls.env["parking.domain"]
        cls.parked = Domain.create({"name": "parked.example", "title": "Hello <b>x</b>", "message": "Registered."})
        cls.redirect = Domain.create(
            {"name": "redir.example", "behavior": "redirect", "redirect_url": "https://target.example/base",
             "preserve_path": True}
        )
        cls.plain_redirect = Domain.create(
            {"name": "plain.example", "behavior": "redirect", "redirect_url": "https://target.example/",
             "redirect_code": "302"}
        )
        cls.sale = Domain.create({"name": "sale.example", "behavior": "for_sale", "price_text": "Offers over $5"})
        cls.gone = Domain.create({"name": "gone.example", "behavior": "gone"})
        cls.indexed = Domain.create({"name": "indexed.example", "noindex": False, "cache_ttl": 60})

    def get(self, path, name, **kw):
        headers = dict(VIA_CLOUDFLARE, **kw.pop("headers", {}))
        return self.url_open(path, headers=host(name, **headers), allow_redirects=False, **kw)

    def post(self, data, name):
        return self.url_open(utils.INQUIRY_PATH, data=data, headers=host(name, **VIA_CLOUDFLARE), allow_redirects=False)

    def assert_clean(self, response):
        self.assertNotIn("Set-Cookie", response.headers)
        self.assertNotIn("X-Parking-Response", response.headers)
        self.assertEqual(response.headers["X-Content-Type-Options"], "nosniff")

    # [@ANCHOR: parking:COMM_test_serve_fallback]
    # [@ANCHOR: parking:COMM_test_post_dispatch]
    def test_parked_page(self):
        # Tests [@ANCHOR: parking:COMM_post_dispatch]
        # Tests [@ANCHOR: parking:COMM_serve_fallback]
        response = self.get("/", "parked.example")
        self.assertEqual(response.status_code, 200)
        self.assert_clean(response)
        self.assertIn("&lt;b&gt;x&lt;/b&gt;", response.text)
        self.assertNotIn("<b>x</b>", response.text)
        self.assertIn("default-src 'none'", response.headers["Content-Security-Policy"])
        self.assertIn("s-maxage=3600", response.headers["Cache-Control"])
        self.assertEqual(response.headers["X-Robots-Tag"], "noindex, nofollow")
        self.assertLess(len(response.content), 3000)
        self.assertEqual(self.get("/any/deep/path?x=1", "parked.example").status_code, 200)

    # [@ANCHOR: parking:COMM_test_parking_service_env]
    def test_www_alias_and_port_in_host(self):
        # Tests [@ANCHOR: parking:COMM_parking_service_env]
        self.assertEqual(self.get("/", "www.parked.example").status_code, 200)
        self.assertEqual(self.get("/", "PARKED.example:443").status_code, 200)

    # [@ANCHOR: parking:COMM_test_parking_response]
    def test_unknown_host_is_a_non_cacheable_404(self):
        # Tests [@ANCHOR: parking:COMM_parking_response]
        response = self.get("/", "nobody.example")
        self.assertEqual(response.status_code, 404)
        self.assertEqual(response.headers["Cache-Control"], "no-store")
        self.assert_clean(response)
        self.assertEqual(self.get("/", "10.1.2.3").status_code, 400)

    def test_unknown_host_default_page_policy(self):
        self.env["ir.config_parameter"].set_param("parking.unknown_host_policy", "default_page")
        response = self.get("/", "nobody.example")
        self.assertEqual(response.status_code, 200)
        self.assertIn("nobody.example", response.text)

    # [@ANCHOR: parking:COMM_test_parking_serve]
    def test_robots_and_indexing(self):
        # Tests [@ANCHOR: parking:COMM_parking_serve]
        self.assertEqual(self.get("/robots.txt", "parked.example").text, "User-agent: *\nDisallow: /\n")
        response = self.get("/robots.txt", "indexed.example")
        self.assertEqual(response.text, "User-agent: *\nAllow: /\n")
        page = self.get("/", "indexed.example")
        self.assertNotIn("X-Robots-Tag", page.headers)
        self.assertIn("s-maxage=60", page.headers["Cache-Control"])

    # [@ANCHOR: parking:COMM_test_parking_page]
    def test_redirects_keep_the_target_host(self):
        # Tests [@ANCHOR: parking:COMM_parking_page]
        response = self.get("//evil.example/x?a=1", "redir.example")
        self.assertEqual(response.status_code, 301)
        self.assertEqual(response.headers["Location"], "https://target.example/base/evil.example/x?a=1")
        self.assert_clean(response)
        plain = self.get("/anything?q=1", "plain.example")
        self.assertEqual((plain.status_code, plain.headers["Location"]), (302, "https://target.example/"))

    def test_gone(self):
        response = self.get("/x", "gone.example")
        self.assertEqual(response.status_code, 410)
        self.assert_clean(response)

    # [@ANCHOR: parking:COMM_test_match_guard]
    def test_backend_routes_do_not_exist_on_a_public_host(self):
        # Tests [@ANCHOR: parking:COMM_match_guard]
        for path in ("/odoo", "/web/login", "/jsonrpc", "/xmlrpc/2/common", "/web/database/manager",
                     "/websocket", "/web/session/authenticate", "/json/2/res.users"):
            response = self.get(path, "parked.example")
            self.assertEqual(response.status_code, 200, path)
            self.assertNotIn("password", response.text.lower(), path)
            self.assertIn("Registered.", response.text, path)
            self.assert_clean(response)
            unknown = self.get(path, "nobody.example")
            self.assertEqual(unknown.status_code, 404, path)

    def test_the_operator_reaches_the_backend_only_through_a_host_that_is_not_a_domain_name(self):
        loopback = "127.0.0.1"  # burn-ignore-ssrf-test-value: Host header value, never connected to
        direct = self.url_open("/web/login", headers=host(loopback), allow_redirects=False)
        self.assertEqual(direct.status_code, 200)
        self.assertIn("password", direct.text.lower())
        for name in ("parked.example", "nobody.example"):
            for proof in ({}, {"CF-Ray": "abc123-SJC"}, {"CF-Connecting-IP": "1.2.3.4"}):
                response = self.url_open("/web/login", headers=host(name, **proof), allow_redirects=False)
                self.assertNotIn("password", response.text.lower(), (name, proof))

    def test_a_forged_forwarded_for_does_not_make_a_visitor_the_operator(self):
        forged = {"X-Forwarded-Host": "parked.example", "X-Forwarded-For": "203.0.113.9"}
        response = self.url_open("/web/login", headers=host("parked.example", **dict(forged, **VIA_CLOUDFLARE)),
                                 allow_redirects=False)
        self.assertNotIn("password", response.text.lower())

    def test_other_methods_are_refused(self):
        response = self.url_open("/", headers=host("parked.example", **VIA_CLOUDFLARE), method="DELETE", allow_redirects=False)
        self.assertEqual(response.status_code, 405)
        response = self.url_open("/", data={"a": "b"}, headers=host("parked.example", **VIA_CLOUDFLARE), allow_redirects=False)
        self.assertEqual(response.status_code, 405)

    def token_from(self, name):
        page = self.get("/", name)
        return re.search(r'name="token" value="([^"]+)"', page.text).group(1)

    # [@ANCHOR: parking:COMM_test_inquiry_controller]
    # [@ANCHOR: parking:COMM_test_inquiry_done]
    def test_sale_form_end_to_end(self):
        # Tests [@ANCHOR: parking:COMM_inquiry_done]
        # Tests [@ANCHOR: parking:COMM_inquiry_controller]
        page = self.get("/", "sale.example")
        self.assertEqual(page.status_code, 200)
        self.assertIn("Offers over $5", page.text)
        token = self.token_from("sale.example")
        data = {"token": token, "name": "Ann", "email": "ann@buyer.example", "message": "I offer 9"}
        self.safe_patch_object(utils, "TOKEN_MIN_AGE", 0)
        response = self.post(data, "sale.example")
        self.assertEqual(response.status_code, 303)
        self.assertEqual(response.headers["Location"], "/?sent=1")
        self.assert_clean(response)
        inquiry = self.env["parking.inquiry"].search([("domain_id", "=", self.sale.id)])
        self.assertEqual(inquiry.email, "ann@buyer.example")
        self.assertTrue(inquiry.ip_hash)
        self.assertIn("Thank you", self.get("/?sent=1", "sale.example").text)

    def test_sale_form_rejects_bots_and_bad_hosts(self):
        token = self.token_from("sale.example")
        base = {"token": token, "email": "bot@x.example", "message": "spam"}
        count = self.env["parking.inquiry"].search_count([])
        self.safe_patch_object(utils, "TOKEN_MIN_AGE", 0)
        self.assertEqual(self.post(dict(base, website="http://spam"), "sale.example").status_code, 303)  # honeypot
        self.assertEqual(self.post(dict(base, token="1.deadbeef"), "sale.example").status_code, 303)  # bad token
        self.assertEqual(self.post(base, "parked.example").status_code, 404)  # not a for-sale domain
        self.assertEqual(self.env["parking.inquiry"].search_count([]), count)

    def test_sale_form_too_soon_after_the_page_is_rendered_stores_nothing(self):
        token = self.token_from("sale.example")
        count = self.env["parking.inquiry"].search_count([])
        response = self.post({"token": token, "email": "a@b.example", "message": "fast"}, "sale.example")
        self.assertEqual(response.status_code, 303)
        self.assertEqual(self.env["parking.inquiry"].search_count([]), count)

    # [@ANCHOR: parking:COMM_test_parking_secret]
    # [@ANCHOR: parking:COMM_test_inquiry_rate_limited]
    def test_sale_form_rate_limit_per_address(self):
        # Tests [@ANCHOR: parking:COMM_parking_secret]
        # Tests [@ANCHOR: parking:COMM_inquiry_rate_limited]
        token = self.token_from("sale.example")
        data = {"token": token, "email": "a@b.example", "message": "hello"}
        self.safe_patch_object(utils, "TOKEN_MIN_AGE", 0)
        codes = [self.post(data, "sale.example").status_code for _ in range(7)]
        self.assertEqual(codes, [303] * 5 + [429] * 2)

    def test_get_on_the_inquiry_path_is_just_a_parked_page(self):
        response = self.get(utils.INQUIRY_PATH, "sale.example")
        self.assertEqual(response.status_code, 200)
        self.assertIn("<form", response.text)

    # [@ANCHOR: parking:COMM_test_extra_kind]
    # [@ANCHOR: parking:COMM_test_public_route]
    def test_only_parked_hostnames_are_classified_as_parking(self):
        # Tests [@ANCHOR: parking:COMM_extra_kind]
        # Tests [@ANCHOR: parking:COMM_public_route]
        ir_http = self.env.registry["ir.http"]
        with MockRequest(self.env):
            self.assertEqual(ir_http._tenant_extra_kind("parked.example"), "parking")
            self.assertEqual(ir_http._tenant_extra_kind("www.parked.example"), "parking")
            self.assertIsNone(ir_http._tenant_extra_kind("nobody.example"))
        self.assertTrue(ir_http._tenant_public_route("parking", utils.INQUIRY_PATH, "POST"))
        self.assertFalse(ir_http._tenant_public_route("parking", utils.INQUIRY_PATH, "GET"))
        self.assertFalse(ir_http._tenant_public_route("unknown", utils.INQUIRY_PATH, "POST"))
        self.assertFalse(ir_http._tenant_public_route("tenant", "/x", "POST"))

    # [@ANCHOR: parking:COMM_test_serve_other]
    def test_a_parked_host_never_shows_the_main_site(self):
        # Tests [@ANCHOR: parking:COMM_serve_other]
        main = self.get("/", "main.parking-test.example")
        self.assertEqual(main.status_code, 200)
        self.assertNotIn("Registered.", main.text)
        for path in ("/", "/privacy", "/web/login", "/blog", "/contactus", "/shop"):
            response = self.get(path, "parked.example")
            self.assertIn("Registered.", response.text, path)
            self.assertNotIn("password", response.text.lower(), path)
        self.assertEqual(self.get("/", "other.main.parking-test.example").status_code, 200)
