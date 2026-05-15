# -*- coding: utf-8 -*-
from odoo.tests.common import HttpCase, tagged
from lxml import etree


@tagged("post_install", "-at_install")
class TestServiceWorker(HttpCase):

    def test_01_sw_headers(self):
        # [@ANCHOR: test_service_worker_01]
        # Tests [@ANCHOR: caching_sw_serve_route]
        """
        Verify that the /sw.js route serves the JavaScript file
        with strict no-cache headers. This guarantees that when
        the module is updated, browsers instantly download the
        new worker rather than relying on a stale cache.
        """
        response = self.url_open("/sw.js")

        # Verify successful routing
        self.assertEqual(
            response.status_code, 200, "The /sw.js route must return a 200 OK."
        )

        # Verify correct MIME type so the browser accepts it as a Service Worker
        content_type = response.headers.get("Content-Type", "")
        self.assertIn(
            "application/javascript",
            content_type,
            "The response must be served as application/javascript.",
        )

        # Verify the critical anti-caching headers
        cache_control = response.headers.get("Cache-Control", "")
        self.assertIn(
            "no-cache", cache_control, "Cache-Control MUST contain 'no-cache'."
        )
        self.assertIn(
            "max-age=0", cache_control, "Cache-Control MUST contain 'max-age=0'."
        )

    def test_02_xpath_rendering_layout(self):
        # [@ANCHOR: test_xpath_rendering_caching_layout]
        # Tests [@ANCHOR: xpath_rendering_caching_layout]
        """Verify the Service Worker script injects into the layout head."""

        view = self.env.ref("website.layout")
        arch = view.with_context(lang=None)._get_combined_arch()
        arch_str = etree.tostring(arch, encoding="unicode")

        self.assertIn(
            "register.js",
            arch_str,
            "The SW registration script must be injected into the compiled layout.",
        )
