# -*- coding: utf-8 -*-
# Copyright © HAMS project. AGPL-3.0-or-later.
from unittest.mock import MagicMock

import redis

from odoo.tests.common import tagged
from odoo.addons.zero_sudo.tests.common import HamsHttpCase


@tagged("post_install", "-at_install")
class TestServiceWorker(HamsHttpCase):

    def test_01_sw_headers(self):
        # [@ANCHOR: COMM_test_service_worker_01]

        # Tests [@ANCHOR: COMM_caching_sw_serve_route]
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

    def test_01b_sw_still_serves_when_redis_is_unreachable(self):
        # Tests [@ANCHOR: COMM_caching_sw_serve_route]
        #
        # Bug-hunt fix (2026-09-27, tier-1 pass): this route's two Redis calls
        # (r.get / r.setex) were the ONLY unguarded get_redis_connection() call
        # site in the codebase -- every other one (redis_cache.py's
        # @distributed_cache read and write, invalidate_model_cache,
        # poll_and_clear_local_cache, distributed_cache_config.check_redis_status)
        # catches redis.RedisError and degrades. Redis here is a cache in front of
        # a file on disk that is the real source of truth, so an unreachable Redis
        # must not turn the site's public, unauthenticated, root-scope /sw.js into
        # a 500 for every visitor while the rest of the site keeps serving.
        broken = MagicMock()
        broken.get.side_effect = redis.exceptions.ConnectionError(
            "simulated Redis outage"
        )
        broken.setex.side_effect = redis.exceptions.ConnectionError(
            "simulated Redis outage"
        )
        self.safe_patch(
            "odoo.addons.caching.controllers.main.get_redis_connection",
            new=lambda env=None: broken,
        )

        response = self.url_open("/sw.js")

        self.assertEqual(
            response.status_code,
            200,
            "[!] DIAGNOSTIC FOR AI: /sw.js must still serve the on-disk template "
            "when Redis is unreachable. A 500 here means the Redis get/setex "
            "calls in ServiceWorkerController.service_worker() are unguarded "
            "again.",
        )
        body = response.content.decode("utf-8")
        self.assertIn("application/javascript", response.headers.get("Content-Type", ""))
        # The placeholder substitution still has to happen on the disk-read path,
        # or the served worker is literally unusable even though it is a 200.
        self.assertNotIn("__CACHE_NAME__", body)
        self.assertNotIn("__MAX_FILE_SIZE_BYTES__", body)
        self.assertNotIn("__MAX_STORAGE_BYTES__", body)
        self.assertNotIn("__TEST_HOOKS_ENABLED__", body)
        broken.get.assert_called_once_with("caching_sw_js_content")

    def test_02_offline_fallback_page_renders(self):
        # Tests [@ANCHOR: COMM_test_caching_pwa_offline_view]

        # Tests [@ANCHOR: caching:COMM_pwa_offline_route]
        response = self.url_open("/offline")
        self.assertEqual(response.status_code, 200)
        self.assertIn(b"You are offline", response.content)

