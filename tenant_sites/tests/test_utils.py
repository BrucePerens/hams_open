# SPDX-License-Identifier: AGPL-3.0-or-later
from odoo.addons.zero_sudo.tests.common import HamsTransactionCase
from odoo.tests import tagged

from .. import utils


@tagged("post_install", "-at_install", "tenant_sites")
class TestTenantSitesUtils(HamsTransactionCase):
    # [@ANCHOR: tenant_sites:COMM_test_normalize_host]
    def test_normalize_host(self):
        # Tests [@ANCHOR: tenant_sites:COMM_normalize_host]
        self.assertEqual(utils.normalize_host("Example.COM"), "example.com")
        self.assertEqual(utils.normalize_host("example.com:8069"), "example.com")
        self.assertEqual(utils.normalize_host("example.com."), "example.com")
        self.assertEqual(utils.normalize_host("bücher.example"), "xn--bcher-kva.example")
        for bad in ("", None, "10.1.2.3", "[::1]", "a b.example", "-a.example", "x.example:abc",
                    "a" * 64 + ".example", 5):
            self.assertEqual(utils.normalize_host(bad), "", bad)
        self.assertEqual(utils.normalize_host("localhost"), "")  # burn-ignore-ssrf-test-value: classification input

    # [@ANCHOR: tenant_sites:COMM_test_original_host]
    def test_original_host_ignores_the_forwarded_rewrite(self):
        # Tests [@ANCHOR: tenant_sites:COMM_original_host]
        environ = {"HTTP_HOST": "b.example", "werkzeug.proxy_fix.orig": {"HTTP_HOST": "a.example"}}
        self.assertEqual(utils.original_host(environ), "a.example")
        self.assertEqual(utils.original_host({"HTTP_HOST": "c.example"}), "c.example")
        self.assertEqual(utils.original_host({}), "")

    # [@ANCHOR: tenant_sites:COMM_test_host_matches]
    # [@ANCHOR: tenant_sites:COMM_test_valid_host_pattern]
    def test_host_patterns(self):
        # Tests [@ANCHOR: tenant_sites:COMM_host_matches]
        # Tests [@ANCHOR: tenant_sites:COMM_valid_host_pattern]
        patterns = ("example.com", "*.example.com")
        self.assertTrue(utils.host_matches("example.com", patterns))
        self.assertTrue(utils.host_matches("a.example.com", patterns))
        self.assertTrue(utils.host_matches("a.b.example.com", patterns))
        self.assertFalse(utils.host_matches("notexample.com", patterns))
        self.assertFalse(utils.host_matches("example.com.evil.example", patterns))
        self.assertFalse(utils.host_matches("example.com", ("*.example.com",)))
        self.assertFalse(utils.host_matches("anything.example", ()))
        self.assertEqual(utils.normalize_host_pattern(" *.Example.com "), "*.example.com")
        self.assertEqual(utils.normalize_host_pattern("Example.com"), "example.com")
        self.assertEqual(utils.normalize_host_pattern("*.example.com.."), "*.example.com")
        self.assertEqual(utils.normalize_host_pattern("*."), "")
        self.assertEqual(utils.normalize_host_pattern("*"), "")

    # [@ANCHOR: tenant_sites:COMM_test_tenant_path_allowed]
    def test_tenant_path_allow_list(self):
        # Tests [@ANCHOR: tenant_sites:COMM_tenant_path_allowed]
        allowed = ("/", "/blog", "/blog/news-1/post-2", "/web/assets/1/abc/web.assets_frontend.min.css",
                   "/web/image/12/image_1024", "/web/content/5", "/web/static/lib/x.js",
                   "/website/static/src/img/a.png", "/theme_x/static/a.css", "/robots.txt", "/sitemap.xml",
                   "/sitemap-1.xml", "/favicon.ico")
        for path in allowed:
            self.assertTrue(utils.tenant_path_allowed(path), path)
        refused = ["/odoo", "/odoo/action-1", "/jsonrpc", "/xmlrpc/2/common", "/json/2/res.users", "/websocket",
                   "/website/info", "/website/force/1", "/@/", "/my", "/my/home", "/shop", "/blogger", "/about",
                   "/longpolling/poll", "/mail/x", "/robots.txt/x", ""]
        # The /web/ backend paths, each tagged: they are test data for the allow-list, not navigation.
        refused.append("/web")  # burn-ignore-route: refused-path test data
        refused.append("/web/login")
        refused.append("/web/database/manager")
        refused.append("/web/session/x")
        refused.append("/web/dataset/call_kw")
        refused.append("/web/webclient/version_info")  # burn-ignore-route: refused-path test data
        for path in refused:
            self.assertFalse(utils.tenant_path_allowed(path), path)

    # [@ANCHOR: tenant_sites:COMM_test_tenant_module_allowed]
    def test_tenant_module_allow_list(self):
        # Tests [@ANCHOR: tenant_sites:COMM_tenant_module_allowed]
        self.assertTrue(utils.tenant_module_allowed("/", "odoo.addons.website.controllers.main"))
        self.assertFalse(utils.tenant_module_allowed("/", "odoo.addons.web.controllers.home"))
        self.assertFalse(utils.tenant_module_allowed("/robots.txt", "odoo.addons.web.controllers.home"))
        self.assertTrue(utils.tenant_module_allowed("/blog", "odoo.addons.website_blog.controllers.main"))
        self.assertTrue(utils.tenant_module_allowed("/web/image/1", "odoo.addons.web.controllers.binary"))
        self.assertFalse(utils.tenant_module_allowed("/blog", "odoo.addons.user_websites.controllers.main"))
        self.assertFalse(utils.tenant_module_allowed("/blog", "odoo.addons.ham_shack.controllers.main"))
        self.assertFalse(utils.tenant_module_allowed("/blog", ""))
        self.assertFalse(utils.tenant_module_allowed("/blog", "odoo.addons.website_sale.controllers.main"))
