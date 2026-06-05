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
        self.assertEqual(
            response.status_code,
            200,
            "[!] DIAGNOSTIC FOR AI: Failed to fetch /sw.js.",
        )
        self.assertIn(
            "fetch",
            response.text,
            "[!] DIAGNOSTIC FOR AI: The Service Worker must contain a fetch listener.",
        )
        self.assertIn(
            "CACHE_URL_REGEX.test",
            response.text,
            "[!] DIAGNOSTIC FOR AI: The Service Worker must utilize CACHE_URL_REGEX for interception.",
        )

    def test_02_sw_regex_logic(self):
        """Verify the regex logic in the Service Worker."""
        response = self.url_open("/sw.js")
        content = response.text
        # Extract regex from sw.js
        match = re.search(r"const CACHE_URL_REGEX = (/.+/);", content)
        self.assertTrue(
            match,
            "[!] DIAGNOSTIC FOR AI: Could not find CACHE_URL_REGEX definition in sw.js.",
        )
        if match:
            regex_str = match.group(1).strip("/")
            # Use Python's re to test the same regex
            pattern = re.compile(regex_str)
            self.assertTrue(
                pattern.search("/web/assets/debug/web.assets_backend.js"),
                "[!] DIAGNOSTIC FOR AI: CACHE_URL_REGEX failed to match /web/assets path.",
            )  # burn-ignore-route
            self.assertTrue(
                pattern.search("/web/assets/12345/web.assets_frontend.css"),
                "[!] DIAGNOSTIC FOR AI: CACHE_URL_REGEX failed to match /web/assets hashed path.",
            )  # burn-ignore-route
            self.assertTrue(
                pattern.search("/my_module/static/src/js/script.js"),
                "[!] DIAGNOSTIC FOR AI: CACHE_URL_REGEX failed to match module static path.",
            )
            self.assertFalse(
                pattern.search("/odoo/image/123"),
                "[!] DIAGNOSTIC FOR AI: CACHE_URL_REGEX incorrectly matched /odoo/image path.",
            )  # burn-ignore-route
            self.assertFalse(
                pattern.search("/api/v1/data"),
                "[!] DIAGNOSTIC FOR AI: CACHE_URL_REGEX incorrectly matched /api path.",
            )
            self.assertFalse(
                pattern.search("/odoo/content/123/file.pdf"),
                "[!] DIAGNOSTIC FOR AI: CACHE_URL_REGEX should not match /odoo/content.",
            )
            self.assertFalse(
                pattern.search("/other/path/web/assets/test.js"),
                "[!] DIAGNOSTIC FOR AI: CACHE_URL_REGEX should only match at the beginning of the path.",
            )
