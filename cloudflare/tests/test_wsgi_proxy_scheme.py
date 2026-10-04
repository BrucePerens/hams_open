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
    # Also the real exercise of the whole-module fix the docstring's own
    # `Verified by [@ANCHOR: test_wsgi_proxy_scheme_fix_sets_https_for_trusted_cf_visitor]`
    # already names this test for -- this is the one case where the CF-Visitor
    # scheme is actually trusted and applied, i.e. the module's own reason to exist.
    # Tests [@ANCHOR: cloudflare:wsgi_proxy_scheme_fix]
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

    # Bug-hunt fix (night_shift_todo/low/cloudflare-trusted-ip-allow-list-defaults-to-cloudflare-
    # ranges-on-tunnel-only-deployments-7d2b8f14.md): Redis being unreachable with no prior
    # successful read (cold start, or a Tunnel-only deployment that never needed Redis for this
    # at all) must NOT fall back to trusting Cloudflare's full published-range snapshot -- that
    # was the bug. It must stay loopback-only, the correct default.
    def test_05d_redis_unavailable_with_no_prior_cache_stays_loopback_only(self):
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
        self.assertEqual(
            result["wsgi.url_scheme"], "http",
            "A Tunnel-only deployment (or any deployment that has never published a real "
            "non-Tunnel allow-list) must never trust a Cloudflare-range peer just because "
            "Redis happens to be unreachable.",
        )
        self.assertNotIn("HTTP_X_FORWARDED_PROTO", result)

    def test_05e_redis_outage_after_a_successful_read_keeps_serving_the_last_known_good_list(self):
        """Once a self-hosted non-Tunnel admin's real allow-list has actually been read from
        Redis, a later transient Redis outage must not drop that trust back to loopback-only --
        it should keep serving the last-known-good list until Redis is reachable again."""
        fake_redis = MagicMock()
        fake_redis.get.return_value = json.dumps(["203.0.113.0/24"])
        self.safe_patch(
            "odoo.addons.cloudflare.wsgi_proxy_scheme.get_redis_connection",
            return_value=fake_redis,
        )
        trusted_environ = {
            "REMOTE_ADDR": "203.0.113.42",  # burn-ignore-ssrf-test-value
            "HTTP_CF_VISITOR": '{"scheme":"https"}',
            "wsgi.url_scheme": "http",
        }
        self.assertEqual(self._call(dict(trusted_environ))["wsgi.url_scheme"], "https")

        # Force the in-process cache to expire, then make Redis start failing.
        wsgi_proxy_scheme._ranges_cache["loaded_at"] = 0.0
        self.safe_patch(
            "odoo.addons.cloudflare.wsgi_proxy_scheme.get_redis_connection",
            side_effect=ConnectionError("redis unreachable"),
        )
        result = self._call(dict(trusted_environ))
        self.assertEqual(
            result["wsgi.url_scheme"], "https",
            "A transient Redis outage must not undo an already-published non-Tunnel allow-list.",
        )

    # Tests [@ANCHOR: cloudflare:wsgi_proxy_scheme_fix_call]
    def test_05_existing_x_forwarded_proto_wins_over_cf_visitor(self):
        """An X-Forwarded-Proto from a trusted peer is authoritative: CF-Visitor is only the fallback
        for when it is absent, and the header itself is left as it arrived."""
        environ = {
            "REMOTE_ADDR": "127.0.0.1",  # burn-ignore-ssrf-test-value
            "HTTP_CF_VISITOR": '{"scheme":"https"}',
            "HTTP_X_FORWARDED_PROTO": "http",
            "wsgi.url_scheme": "http",
        }
        result = self._call(environ)
        self.assertEqual(result["wsgi.url_scheme"], "http")
        self.assertEqual(result["HTTP_X_FORWARDED_PROTO"], "http")

    # Tests [@ANCHOR: cloudflare:wsgi_proxy_scheme_fix_call]
    def test_06_real_edge_sends_both_headers_and_https_is_applied(self):
        """The real Cloudflare edge sends Cf-Visitor AND X-Forwarded-Proto together (captured off
        hams.com's loopback, 2026-09-28). Odoo's ProxyFix ignores the latter without
        X-Forwarded-Host, so the scheme has to be applied here; before this fix the session cookie
        went out without Secure on the live site."""
        environ = {
            "REMOTE_ADDR": "127.0.0.1",  # burn-ignore-ssrf-test-value
            "HTTP_CF_VISITOR": '{"scheme":"https"}',
            "HTTP_X_FORWARDED_PROTO": "https",
            "wsgi.url_scheme": "http",
        }
        result = self._call(environ)
        self.assertEqual(result["wsgi.url_scheme"], "https")
        self.assertEqual(result["HTTP_X_FORWARDED_PROTO"], "https")

    # Tests [@ANCHOR: cloudflare:wsgi_proxy_scheme_fix_call]
    def test_07_x_forwarded_proto_alone_and_a_comma_list_are_applied(self):
        alone = self._call({"REMOTE_ADDR": "127.0.0.1", "HTTP_X_FORWARDED_PROTO": "https", "wsgi.url_scheme": "http"})  # burn-ignore-ssrf-test-value
        self.assertEqual(alone["wsgi.url_scheme"], "https")
        chained = self._call({"REMOTE_ADDR": "127.0.0.1", "HTTP_X_FORWARDED_PROTO": "https, http", "wsgi.url_scheme": "http"})  # burn-ignore-ssrf-test-value
        self.assertEqual(chained["wsgi.url_scheme"], "https")

    # Tests [@ANCHOR: cloudflare:wsgi_proxy_scheme_fix_call]
    def test_08_untrusted_peer_x_forwarded_proto_is_ignored(self):
        environ = {"REMOTE_ADDR": "203.0.113.7", "HTTP_X_FORWARDED_PROTO": "https", "wsgi.url_scheme": "http"}  # burn-ignore-ssrf-test-value
        self.assertEqual(self._call(environ)["wsgi.url_scheme"], "http")

    # Tests [@ANCHOR: cloudflare:wsgi_proxy_scheme_fix_call]
    def test_09_garbage_x_forwarded_proto_falls_back_to_cf_visitor(self):
        environ = {
            "REMOTE_ADDR": "127.0.0.1",  # burn-ignore-ssrf-test-value
            "HTTP_X_FORWARDED_PROTO": "gopher",
            "HTTP_CF_VISITOR": '{"scheme":"https"}',
            "wsgi.url_scheme": "http",
        }
        self.assertEqual(self._call(environ)["wsgi.url_scheme"], "https")

    # Tests [@ANCHOR: cloudflare:strip_client_forwarded_host]
    def test_10_client_x_forwarded_host_is_dropped_on_a_cloudflare_request(self):
        """A visitor's X-Forwarded-Host would make Odoo's ProxyFix replace Host; it is removed on
        every request that carries a Cloudflare marker header, the scheme still applied."""
        for marker in ("HTTP_CF_RAY", "HTTP_CF_CONNECTING_IP", "HTTP_CF_VISITOR"):
            environ = {
                "REMOTE_ADDR": "127.0.0.1",  # burn-ignore-ssrf-test-value
                "HTTP_HOST": "hams.com",
                "HTTP_X_FORWARDED_HOST": "crawler.hams.com",
                "HTTP_X_FORWARDED_PROTO": "https",
                marker: '{"scheme":"https"}' if marker == "HTTP_CF_VISITOR" else "x",
                "wsgi.url_scheme": "http",
            }
            result = self._call(environ)
            self.assertNotIn("HTTP_X_FORWARDED_HOST", result, marker)
            self.assertEqual(result["HTTP_HOST"], "hams.com")
            self.assertEqual(result["wsgi.url_scheme"], "https")

    # Tests [@ANCHOR: cloudflare:strip_client_forwarded_host]
    def test_11_x_forwarded_host_is_kept_without_a_cloudflare_marker(self):
        """A local reverse proxy that is not Cloudflare (the dev nginx site) sets the header itself."""
        environ = {
            "REMOTE_ADDR": "127.0.0.1",  # burn-ignore-ssrf-test-value
            "HTTP_X_FORWARDED_HOST": "hams.com",
            "wsgi.url_scheme": "http",
        }
        self.assertEqual(self._call(environ)["HTTP_X_FORWARDED_HOST"], "hams.com")

    # Tests [@ANCHOR: cloudflare:strip_client_forwarded_host]
    def test_12_untrusted_peer_cloudflare_request_also_loses_x_forwarded_host(self):
        environ = {
            "REMOTE_ADDR": "203.0.113.7",  # burn-ignore-ssrf-test-value
            "HTTP_CF_RAY": "abc",
            "HTTP_X_FORWARDED_HOST": "evil.example",
            "wsgi.url_scheme": "http",
        }
        self.assertNotIn("HTTP_X_FORWARDED_HOST", self._call(environ))
