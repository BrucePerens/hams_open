# -*- coding: utf-8 -*-
from unittest.mock import MagicMock
from odoo.tests.common import tagged
from odoo.addons.zero_sudo.tests.common import HamsHttpCase


@tagged("post_install", "-at_install")
class TestRequestContext(HamsHttpCase):
    def test_01_get_request_context(self):
        # [@ANCHOR: test_cf_get_request_context]
        # Tests [@ANCHOR: cf_get_request_context]
        """Verify that Cloudflare headers are correctly parsed into the request context."""

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
        mock_obj.httprequest.remote_addr = "1.1.1.1"
        mock_obj._get_current_object.return_value = mock_obj
        self.safe_patch(
            "odoo.addons.cloudflare.models.edge_context.request", new=mock_obj
        )

        context = self.env["cloudflare.utils"].get_request_context()

        self.assertEqual(context["ip"], "1.2.3.4")
        self.assertEqual(context["country"], "US")
        self.assertEqual(context["city"], "New York")
        self.assertEqual(context["threat_score"], "10")

    def test_02_get_request_context_no_headers(self):
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
