# SPDX-License-Identifier: AGPL-3.0-or-later
from unittest.mock import patch
from urllib.parse import urlsplit

from odoo.addons.zero_sudo.tests.common import HamsHttpCase
from odoo.tools import config
from odoo.tests import tagged

# Every request Cloudflare forwards carries CF-Ray; the tests send it too, so they walk the same
# path as production. Classification depends only on the Host, so a request without CF-Ray for a
# real domain name is classified the same way (that is how an operator checks a site over loopback).
VIA_CLOUDFLARE = {"CF-Ray": "test-SJC"}
LOOPBACK_STYLE_HOSTS = (  # burn-ignore-ssrf-test-value: Host header values, never connected to
    "127.0.0.1",  # burn-ignore-ssrf-test-value
    "localhost",  # burn-ignore-ssrf-test-value
    "odoo",
    "10.99.0.1",
)
TENANT = "tenant.example"
MAIN = "main.example"


@tagged("post_install", "-at_install", "tenant_sites")
class TestTenantSitesHttp(HamsHttpCase):
    @classmethod
    def setUpClass(cls):
        super().setUpClass()
        env = cls.env
        cls.main_website = env["website"].search([], order="sequence, id", limit=1)
        cls.tenant_website = env["website"].create({"name": "Tenant", "domain": f"https://{TENANT}"})
        env["tenant.site"].create(
            {
                "name": "Tenant",
                "website_id": cls.tenant_website.id,
                "host_ids": [(0, 0, {"name": TENANT}), (0, 0, {"name": f"www.{TENANT}"})],
            }
        )
        env["tenant.site.own.host"].create([{"name": MAIN}, {"name": f"*.{MAIN}"}])
        cls.tenant_page = cls._page("/tenant-only", "TENANT-ONLY-CONTENT", cls.tenant_website.id)
        cls.main_page = cls._page("/main-only", "MAIN-ONLY-CONTENT", cls.main_website.id)
        cls.shared_page = cls._page("/shared-page", "SHARED-CONTENT", False)
        cls.tenant_blog = env["blog.blog"].create({"name": "Tenant Blog", "website_id": cls.tenant_website.id})
        cls.main_blog = env["blog.blog"].create({"name": "Main Blog", "website_id": cls.main_website.id})
        cls.shared_blog = env["blog.blog"].create({"name": "Shared Blog", "website_id": False})
        posts = {}
        for blog, title in ((cls.tenant_blog, "Tenant Post"), (cls.main_blog, "Main Post"),
                            (cls.shared_blog, "Shared Post")):
            posts[title] = env["blog.post"].create({"name": title, "blog_id": blog.id, "is_published": True})
        cls.tenant_post, cls.main_post, cls.shared_post = posts.values()
        env["website.rewrite"].create(
            {"name": "tenant redirect", "url_from": "/old", "url_to": "/tenant-only", "redirect_type": "301",
             "website_id": cls.tenant_website.id}
        )
        env["website.rewrite"].create(
            {"name": "shared redirect", "url_from": "/shared-old", "url_to": "/tenant-only", "redirect_type": "301",
             "website_id": False}
        )

    @classmethod
    def _page(cls, url, text, website_id):
        view = cls.env["ir.ui.view"].create(
            {
                "name": f"test {url}",
                "type": "qweb",
                "key": f"tenant_sites.test_{url.strip('/').replace('-', '_')}",
                "arch_db": f'<t t-call="website.layout"><div id="wrap">{text}</div></t>',
            }
        )
        return cls.env["website.page"].create(
            {"url": url, "is_published": True, "website_id": website_id, "view_id": view.id}
        )

    def get(self, path, host, **kw):
        headers = dict(VIA_CLOUDFLARE, Host=host, **kw.pop("headers", {}))
        return self.url_open(path, headers=headers, allow_redirects=False, **kw)

    # [@ANCHOR: tenant_sites:COMM_test_match_guard]
    # [@ANCHOR: tenant_sites:COMM_test_tenant_match]
    def test_tenant_serves_its_own_page_and_nothing_of_the_main_site(self):
        # Tests [@ANCHOR: tenant_sites:COMM_match_guard]
        # Tests [@ANCHOR: tenant_sites:COMM_tenant_match]
        for host in (TENANT, f"www.{TENANT}", f"WWW.{TENANT}:443"):
            response = self.get("/tenant-only", host)
            self.assertEqual(response.status_code, 200, host)
            self.assertIn("TENANT-ONLY-CONTENT", response.text)
        self.assertEqual(self.get("/main-only", TENANT).status_code, 404)
        self.assertEqual(self.get("/shared-page", TENANT).status_code, 404)
        self.assertEqual(self.get("/privacy", TENANT).status_code, 404)
        self.assertEqual(self.get("/tenant-only", MAIN).status_code, 404)
        self.assertEqual(self.get("/main-only", MAIN).status_code, 200)
        self.assertEqual(self.get("/shared-page", MAIN).status_code, 200)

    # [@ANCHOR: tenant_sites:COMM_test_scoped_page_info]
    # [@ANCHOR: tenant_sites:COMM_test_serve_redirect]
    def test_tenant_redirects_are_its_own(self):
        # Tests [@ANCHOR: tenant_sites:COMM_scoped_page_info]
        # Tests [@ANCHOR: tenant_sites:COMM_serve_redirect]
        own = self.get("/old", TENANT)
        self.assertEqual(own.status_code, 301)
        self.assertEqual(self.get("/shared-old", TENANT).status_code, 404)

    def test_tenant_blog_shows_only_its_own_posts(self):
        response = self.get("/blog", TENANT)
        if response.status_code == 302:  # a website with one blog goes straight to it
            response = self.get(urlsplit(response.headers["Location"]).path, TENANT)
        self.assertEqual(response.status_code, 200)
        self.assertIn("Tenant Post", response.text)
        self.assertNotIn("Main Post", response.text)
        self.assertNotIn("Shared Post", response.text)
        main_response = self.get("/blog", MAIN)
        self.assertIn("Main Post", main_response.text)
        self.assertIn("Shared Post", main_response.text)
        self.assertNotIn("Tenant Post", main_response.text)

    # [@ANCHOR: tenant_sites:COMM_test_scoped_search]
    # [@ANCHOR: tenant_sites:COMM_test_scoped_access]
    def test_a_post_named_in_the_url_must_belong_to_the_tenant_website(self):
        # Tests [@ANCHOR: tenant_sites:COMM_scoped_search]
        # Tests [@ANCHOR: tenant_sites:COMM_scoped_access]
        own = self.get(self.tenant_post.website_url, TENANT)
        self.assertEqual(own.status_code, 200)
        self.assertIn("Tenant Post", own.text)
        for post in (self.shared_post, self.main_post):
            refused = self.get(post.website_url, TENANT)
            self.assertEqual(refused.status_code, 404, post.website_url)
            self.assertNotIn(post.name, refused.text)
        # Outside a tenant request nothing is filtered: the same lookups see every blog.
        self.assertEqual(len(self.env["blog.blog"].search([("id", "in", [self.tenant_blog.id, self.shared_blog.id])])), 2)
        self.assertEqual(self.get(self.shared_post.website_url, MAIN).status_code, 200)

    def test_the_backend_and_login_do_not_exist_on_a_tenant_host(self):
        paths = ["/odoo", "/odoo/action-1", "/jsonrpc", "/xmlrpc/2/common", "/xmlrpc/2/object",
                 "/json/2/res.users", "/websocket", "/longpolling/poll", "/website/info", "/website/force/1",
                 "/@/", "/my", "/my/home", "/shop", "/forum", "/event", "/mail/thread/messages",
                 "/web/static/../../etc/passwd"]
        # Backend paths, each tagged: they are refused-path test data, not navigation targets.
        paths.append("/web")  # burn-ignore-route: refused-path test data
        paths.append("/web/login")
        paths.append("/web/signup")
        paths.append("/web/database/manager")
        paths.append("/web/session/authenticate")
        paths.append("/web/dataset/call_kw")
        paths.append("/web/reset_password")  # burn-ignore-route: refused-path test data
        paths.append("/web/become")  # burn-ignore-route: refused-path test data
        paths.append("/web/webclient/version_info")  # burn-ignore-route: refused-path test data
        for path in paths:
            response = self.get(path, TENANT)
            self.assertEqual(response.status_code, 404, path)
            self.assertNotIn("password", response.text.lower(), path)
            self.assertNotIn("Odoo", response.headers.get("Server", ""), path)

    def test_a_tenant_host_is_read_only(self):
        for method in ("POST", "PUT", "DELETE", "PATCH"):
            for path in ("/", "/tenant-only", "/web/login", "/jsonrpc", "/website/form/res.partner",
                         "/web/dataset/call_kw"):
                response = self.url_open(
                    path, data={"a": "b"} if method == "POST" else None,
                    headers=dict(VIA_CLOUDFLARE, Host=TENANT), method=method, allow_redirects=False,
                )
                self.assertEqual(response.status_code, 404, (method, path))

    def test_public_assets_and_robots_still_work_on_a_tenant_host(self):
        self.assertEqual(self.get("/robots.txt", TENANT).status_code, 200)
        self.assertEqual(self.get("/sitemap.xml", TENANT).status_code, 200)
        self.assertEqual(self.get("/", TENANT).status_code, 200)
        self.assertEqual(self.get("/web/static/img/odoo-icon-ios.png", TENANT).status_code, 200)

    # [@ANCHOR: tenant_sites:COMM_test_classify]
    def test_forged_forwarded_host_is_rejected(self):
        # Tests [@ANCHOR: tenant_sites:COMM_classify]
        # Odoo's proxy_mode replaces Host by X-Forwarded-Host; the router uses the Host the client sent
        # and refuses a request where the two differ. (On the live site the cloudflare module also
        # drops the header from every request that carries Cloudflare's own headers.)
        with patch.dict(config.options, {"proxy_mode": True}):
            for host, forged in ((TENANT, MAIN), (MAIN, TENANT), ("nobody.example", MAIN)):
                response = self.url_open(
                    "/main-only", headers={"Host": host, "X-Forwarded-Host": forged}, allow_redirects=False
                )
                self.assertEqual(response.status_code, 400, (host, forged))
                self.assertNotIn("MAIN-ONLY-CONTENT", response.text)
            same = self.url_open(
                "/main-only", headers={"Host": MAIN, "X-Forwarded-Host": MAIN}, allow_redirects=False
            )
            self.assertEqual(same.status_code, 200)

    # [@ANCHOR: tenant_sites:COMM_test_serve_fallback]
    # [@ANCHOR: tenant_sites:COMM_test_serve_other]
    # [@ANCHOR: tenant_sites:COMM_test_plain_response]
    # [@ANCHOR: tenant_sites:COMM_test_post_dispatch]
    def test_unknown_hosts_get_an_uncached_404_with_no_cookie(self):
        # Tests [@ANCHOR: tenant_sites:COMM_serve_fallback]
        # Tests [@ANCHOR: tenant_sites:COMM_serve_other]
        # Tests [@ANCHOR: tenant_sites:COMM_plain_response]
        # Tests [@ANCHOR: tenant_sites:COMM_post_dispatch]
        for path in ("/", "/main-only", "/web/login", "/jsonrpc", "/websocket"):
            response = self.get(path, "nobody.example")
            self.assertEqual(response.status_code, 404, path)
            self.assertEqual(response.headers["Cache-Control"], "no-store")
            self.assertNotIn("Set-Cookie", response.headers)
            self.assertNotIn("MAIN-ONLY-CONTENT", response.text)
        self.assertEqual(self.get("/", "10.1.2.3").status_code, 400)

    def test_main_hostnames_and_member_custom_domains_are_served_as_before(self):
        self.assertEqual(self.get("/web/login", MAIN).status_code, 200)
        self.assertEqual(self.get("/web/login", f"relay.{MAIN}").status_code, 200)
        self.env["edge.routing.domain"].create({"name": "member.example", "target_slug": "somebody"})
        self.assertEqual(self.get("/web/login", "member.example").status_code, 200)

    def test_requests_for_a_host_that_is_not_a_domain_name_are_unchanged(self):
        # Daemons, the test harness and operators on loopback use localhost or an address.
        for host in LOOPBACK_STYLE_HOSTS:
            response = self.url_open("/web/login", headers={"Host": host}, allow_redirects=False)
            self.assertEqual(response.status_code, 200, host)
            self.assertIn("password", response.text.lower(), host)
        # Through the tunnel such a Host is refused, and a real domain name is classified without CF-Ray.
        self.assertEqual(self.get("/", "10.99.0.1").status_code, 400)
        direct = self.url_open("/web/login", headers={"Host": TENANT}, allow_redirects=False)
        self.assertEqual(direct.status_code, 404)

    def test_nothing_changes_until_main_hostnames_are_configured(self):
        self.env["tenant.site.own.host"].search([]).unlink()
        self.assertEqual(self.get("/main-only", "anything.example").status_code, 200)
        self.assertEqual(self.get("/main-only", TENANT).status_code, 404)

    # [@ANCHOR: tenant_sites:COMM_test_extra_kind]
    # [@ANCHOR: tenant_sites:COMM_test_public_route]
    def test_default_hooks_add_no_route_and_parking_adds_its_kind_only_for_parked_names(self):
        # Tests [@ANCHOR: tenant_sites:COMM_extra_kind]
        # Tests [@ANCHOR: tenant_sites:COMM_public_route]
        ir_http = self.env.registry["ir.http"]
        self.assertFalse(ir_http._tenant_public_route("unknown", "/x", "GET"))
        self.assertFalse(ir_http._tenant_public_route("unknown", "/__parking/inquiry", "GET"))
        self.env["parking.domain"].create({"name": "parked-by-test.example"})
        parked = self.get("/", "parked-by-test.example")
        self.assertEqual(parked.status_code, 200)
        self.assertNotIn("MAIN-ONLY-CONTENT", parked.text)
        self.assertEqual(self.get("/", "not-parked.example").status_code, 404)
