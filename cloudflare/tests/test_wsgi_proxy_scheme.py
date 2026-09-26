# -*- coding: utf-8 -*-
# Copyright © HAMS project. AGPL-3.0-or-later.
import json
from unittest.mock import MagicMock

from odoo.tests.common import tagged
from odoo.addons.zero_sudo.tests.common import HamsTransactionCase

from odoo.addons.cloudflare import wsgi_proxy_scheme
from odoo.addons.cloudflare.wsgi_proxy_scheme import _patched_application_call


@tagged("post_install", "-at_install")
class TestWsgiProxyScheme(HamsTransactionCase):
    """Pure unit coverage for _patched_application_call: no real HTTP request needed,
    since the function under test operates on a plain WSGI environ dict before Odoo's own
    routing ever sees it. `_original_application_call` is mocked out rather than really
    invoked -- that's stock Odoo's own WSGI entry point, not this fix's own logic, and
    calling it for real here would need a whole running app."""

    def setUp(self):
        super().setUp()
        # _ranges_cache is module-level and process-wide (see its own comment: one copy per
        # worker process, refreshed on a TTL) so it survives across test methods and even
        # across test classes in the same suite run -- reset it here so every test gets a
        # deterministic, freshly-read cache instead of whatever a previous test left behind.
        self.addCleanup(
            wsgi_proxy_scheme._ranges_cache.update, {"networks": (), "loaded_at": 0.0}
        )
        wsgi_proxy_scheme._ranges_cache.update({"networks": (), "loaded_at": 0.0})

    def _call(self, environ):
        fake_self = MagicMock()
        start_response = MagicMock()
        mock_original = self.safe_patch(
            "odoo.addons.cloudflare.wsgi_proxy_scheme._original_application_call"
        )
        _patched_application_call(fake_self, environ, start_response)
        mock_original.assert_called_once_with(fake_self, environ, start_response)
        return environ

    # [@ANCHOR: test_wsgi_proxy_scheme_fix_sets_https_for_trusted_cf_visitor]
    # Tests [@ANCHOR: cloudflare:wsgi_proxy_scheme_fix_call]
    def test_01_trusted_loopback_cf_visitor_https_sets_wsgi_url_scheme(self):
        environ = {
            "REMOTE_ADDR": "127.0.0.1",  # burn-ignore-ssrf-test-value
            "HTTP_CF_VISITOR": '{"scheme":"https"}',
            "wsgi.url_scheme": "http",
        }
        result = self._call(environ)
        self.assertEqual(result["wsgi.url_scheme"], "https")
        self.assertEqual(result["HTTP_X_FORWARDED_PROTO"], "https")

    # Tests [@ANCHOR: cloudflare:wsgi_proxy_scheme_fix_call]
    def test_02_non_loopback_peer_forged_cf_visitor_is_ignored(self):
        """Bug-hunt-shaped case, matching cloudflare/models/edge_context.py's own
        loopback trust check: a direct, non-tunneled connection could send any
        CF-Visitor value it likes -- it must not be trusted."""
        environ = {
            "REMOTE_ADDR": "203.0.113.7",  # burn-ignore-ssrf-test-value
            "HTTP_CF_VISITOR": '{"scheme":"https"}',
            "wsgi.url_scheme": "http",
        }
        result = self._call(environ)
        self.assertEqual(result["wsgi.url_scheme"], "http")
        self.assertNotIn("HTTP_X_FORWARDED_PROTO", result)

    # Tests [@ANCHOR: cloudflare:wsgi_proxy_scheme_fix_call]
    def test_03_no_cf_visitor_header_leaves_scheme_untouched(self):
        environ = {
            "REMOTE_ADDR": "127.0.0.1",  # burn-ignore-ssrf-test-value
            "wsgi.url_scheme": "http",
        }
        result = self._call(environ)
        self.assertEqual(result["wsgi.url_scheme"], "http")
        self.assertNotIn("HTTP_X_FORWARDED_PROTO", result)

    # Tests [@ANCHOR: cloudflare:wsgi_proxy_scheme_fix_call]
    def test_04_malformed_cf_visitor_json_does_not_crash_or_change_scheme(self):
        environ = {
            "REMOTE_ADDR": "127.0.0.1",  # burn-ignore-ssrf-test-value
            "HTTP_CF_VISITOR": "not-json",
            "wsgi.url_scheme": "http",
        }
        result = self._call(environ)
        self.assertEqual(result["wsgi.url_scheme"], "http")
        self.assertNotIn("HTTP_X_FORWARDED_PROTO", result)

    # Tests [@ANCHOR: cloudflare:is_trusted_cf_peer]
    def test_05b_non_tunnel_peer_in_the_redis_published_allowlist_is_trusted(self):
        """A self-hosted admin running Cloudflare WITHOUT Tunnel: the peer is a real network
        address (never loopback), but it falls inside the admin-configured trusted-range
        allow-list published to Redis, so its CF-Visitor must be trusted just like the
        Tunnel/loopback case is."""
        fake_redis = MagicMock()
        fake_redis.get.return_value = json.dumps(["203.0.113.0/24"])
        self.safe_patch(
            "odoo.addons.cloudflare.wsgi_proxy_scheme.get_redis_connection",
            return_value=fake_redis,
        )
        environ = {
            "REMOTE_ADDR": "203.0.113.42",  # burn-ignore-ssrf-test-value
            "HTTP_CF_VISITOR": '{"scheme":"https"}',
            "wsgi.url_scheme": "http",
        }
        result = self._call(environ)
        self.assertEqual(result["wsgi.url_scheme"], "https")
        self.assertEqual(result["HTTP_X_FORWARDED_PROTO"], "https")

    def test_05c_non_tunnel_peer_outside_the_allowlist_is_still_untrusted(self):
        fake_redis = MagicMock()
        fake_redis.get.return_value = json.dumps(["203.0.113.0/24"])
        self.safe_patch(
            "odoo.addons.cloudflare.wsgi_proxy_scheme.get_redis_connection",
            return_value=fake_redis,
        )
        environ = {
            "REMOTE_ADDR": "198.51.100.7",  # burn-ignore-ssrf-test-value
            "HTTP_CF_VISITOR": '{"scheme":"https"}',
            "wsgi.url_scheme": "http",
        }
        result = self._call(environ)
        self.assertEqual(result["wsgi.url_scheme"], "http")
        self.assertNotIn("HTTP_X_FORWARDED_PROTO", result)

    def test_05d_redis_unavailable_falls_back_to_the_baked_in_default_ranges(self):
        """A real Cloudflare-range peer (no Tunnel, Redis down/unreachable) must still be
        trusted via the module's own baked-in snapshot -- Redis being down must never
        silently disable the whole allow-list."""
        self.safe_patch(
            "odoo.addons.cloudflare.wsgi_proxy_scheme.get_redis_connection",
            side_effect=ConnectionError("redis unreachable"),
        )
        environ = {
            "REMOTE_ADDR": "173.245.48.1",  # a real Cloudflare-published range
            "HTTP_CF_VISITOR": '{"scheme":"https"}',
            "wsgi.url_scheme": "http",
        }
        result = self._call(environ)
        self.assertEqual(result["wsgi.url_scheme"], "https")

    # Tests [@ANCHOR: cloudflare:wsgi_proxy_scheme_fix_call]
    def test_05_existing_x_forwarded_proto_is_never_overridden(self):
        """If something upstream already set a real X-Forwarded-Proto, this fix must not
        second-guess it -- CF-Visitor is a fallback for this deployment's own gap, not a
        general override."""
        environ = {
            "REMOTE_ADDR": "127.0.0.1",  # burn-ignore-ssrf-test-value
            "HTTP_CF_VISITOR": '{"scheme":"https"}',
            "HTTP_X_FORWARDED_PROTO": "http",
            "wsgi.url_scheme": "http",
        }
        result = self._call(environ)
        self.assertEqual(result["wsgi.url_scheme"], "http")
        self.assertEqual(result["HTTP_X_FORWARDED_PROTO"], "http")
