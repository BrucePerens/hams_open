# -*- coding: utf-8 -*-
from odoo.tests.common import tagged
from odoo.addons.zero_sudo.tests.common import HamsHttpCase


@tagged("post_install", "-at_install")
class TestServiceWorker(HamsHttpCase):

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
            response.status_code,
            200,
            "[!] DIAGNOSTIC FOR AI: The /sw.js route must return a 200 OK. "
            "If it returns 404, the controller binding or the file path "
            "in ServiceWorkerController.service_worker() might be incorrect.",
        )

        # Verify correct MIME type so the browser accepts it as a Service Worker
        content_type = response.headers.get("Content-Type", "")
        self.assertIn(
            "application/javascript",
            content_type,
            "[!] DIAGNOSTIC FOR AI: The response must be served as application/javascript. "
            "Browsers will reject Service Workers with incorrect MIME types.",
        )

        # Verify the critical anti-caching headers
        cache_control = response.headers.get("Cache-Control", "")
        self.assertIn(
            "no-cache",
            cache_control,
            "[!] DIAGNOSTIC FOR AI: Cache-Control MUST contain 'no-cache'. "
            "This ensures the browser checks for a new Service Worker on every load.",
        )
        self.assertIn(
            "max-age=0",
            cache_control,
            "[!] DIAGNOSTIC FOR AI: Cache-Control MUST contain 'max-age=0'. "
            "This prevents the browser from using a stale Service Worker script.",
        )
