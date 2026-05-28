# -*- coding: utf-8 -*-
import re
from odoo.tests.common import tagged
from odoo.addons.zero_sudo.tests.common import HamsHttpCase

@tagged("post_install", "-at_install")
class TestServiceWorkerFetch(HamsHttpCase):

    def test_01_sw_fetch_presence(self):
        # [@ANCHOR: test_sw_fetch_01]
        # Tests [@ANCHOR: caching_sw_fetch_interceptor]
        """Verify the fetch interceptor is present in the Service Worker source."""
        response = self.url_open("/sw.js")
        self.assertEqual(response.status_code, 200)
        self.assertIn("fetch", response.text)
        self.assertIn("CACHE_URL_REGEX.test", response.text)

    def test_02_sw_regex_logic(self):
        """Verify the regex logic in the Service Worker."""
        response = self.url_open("/sw.js")
        content = response.text
        # Extract regex from sw.js
        match = re.search(r'const CACHE_URL_REGEX = (/.+/);', content)
        if match:
            regex_str = match.group(1).strip('/')
            # Use Python's re to test the same regex
            pattern = re.compile(regex_str)
            self.assertTrue(pattern.search("/web/assets/debug/web.assets_backend.js")) # burn-ignore-route
            self.assertTrue(pattern.search("/web/assets/12345/web.assets_frontend.css")) # burn-ignore-route
            self.assertTrue(pattern.search("/my_module/static/src/js/script.js"))
            self.assertFalse(pattern.search("/web/image/123")) # burn-ignore-route
            self.assertFalse(pattern.search("/api/v1/data"))
