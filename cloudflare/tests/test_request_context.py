# This software is distributed under the terms of the Affero General Public License (AGPL-3).

# -*- coding: utf-8 -*-
from unittest.mock import MagicMock
from odoo.tests.common import tagged
from odoo.addons.zero_sudo.tests.common import HamsHttpCase


@tagged("post_install", "-at_install")
class TestRequestContext(HamsHttpCase):
    def test_01_get_request_context_via_loopback_tunnel_trusts_cf_headers(self):
        # [@ANCHOR: COMM_test_cf_get_request_context]

        # Tests [@ANCHOR: COMM_cf_get_request_context]
        """A request whose real transport peer IS the local Cloudflare
        Tunnel (loopback remote_addr) must have its CF-* headers trusted
        -- this is the one deployment shape (Tunnel-only, confirmed by
        Bruce 2026-09-11) where they're genuine."""

        headers = {
            "CF-Connecting-IP": "1.2.3.4",
            "CF-IPCountry": "US",
            "CF-IPCity": "New York",
            "CF-IPLongitude": "-74.006",
            "CF-IPLatitude": "40.7128",
            "CF-Threat-Score": "10",
        }

        # safe_patch replaces the target, so we mock the entire object
        mock_obj = MagicMock()
        mock_obj.httprequest.headers = headers
        mock_obj.httprequest.remote_addr = "127.0.0.1"  # burn-ignore-ssrf-test-value
        mock_obj._get_current_object.return_value = mock_obj
        self.safe_patch(
            "odoo.addons.cloudflare.models.edge_context.request", new=mock_obj
        )

        context = self.env["cloudflare.utils"].get_request_context()

        self.assertEqual(context["ip"], "1.2.3.4")
        self.assertEqual(context["country"], "US")
        self.assertEqual(context["city"], "New York")
        self.assertEqual(context["threat_score"], "10")

    def test_01b_get_request_context_from_a_non_loopback_peer_ignores_forged_cf_headers(self):
        # Tests [@ANCHOR: COMM_cf_get_request_context]
        """Bug-hunt fix, 2026-09-11: a request whose real transport peer is
        NOT the local Tunnel (a direct connection to origin, bypassing
        Cloudflare) must NOT trust any CF-* header -- they're
        attacker-controlled on that path. This is the exact scenario the
        pre-fix code got wrong: it trusted CF-Connecting-IP unconditionally,
        letting a direct caller forge any IP/geo/threat data it wanted."""

        headers = {
            "CF-Connecting-IP": "1.2.3.4",
            "CF-IPCountry": "US",
            "CF-IPCity": "New York",
            "CF-IPLongitude": "-74.006",
            "CF-IPLatitude": "40.7128",
            "CF-Threat-Score": "10",
        }

        mock_obj = MagicMock()
        mock_obj.httprequest.headers = headers
        mock_obj.httprequest.remote_addr = "1.1.1.1"
        mock_obj._get_current_object.return_value = mock_obj
        self.safe_patch(
            "odoo.addons.cloudflare.models.edge_context.request", new=mock_obj
        )

        context = self.env["cloudflare.utils"].get_request_context()

        self.assertEqual(
            context["ip"], "1.1.1.1",
            "A non-loopback peer's own real address must be used, never the "
            "forgeable CF-Connecting-IP header.",
        )
        self.assertIsNone(context["country"])
        self.assertIsNone(context["city"])
        self.assertIsNone(context["threat_score"])

    def test_02_get_request_context_no_headers(self):
        # [@ANCHOR: COMM_test_02_get_request_context_no_headers]
        """Verify fallback when Cloudflare headers are missing."""
        mock_obj = MagicMock()
        mock_obj.httprequest.headers = {}
        mock_obj.httprequest.remote_addr = "1.1.1.1"
        mock_obj._get_current_object.return_value = mock_obj
        self.safe_patch(
            "odoo.addons.cloudflare.models.edge_context.request", new=mock_obj
        )

        context = self.env["cloudflare.utils"].get_request_context()
        self.assertEqual(context["ip"], "1.1.1.1")
        self.assertIsNone(context["country"])
