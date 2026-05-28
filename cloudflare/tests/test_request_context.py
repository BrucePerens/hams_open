# -*- coding: utf-8 -*-
from unittest.mock import MagicMock
from odoo.tests.common import tagged, mock_request
from odoo.addons.hams_test.tests.common import HamsHttpCase

@tagged("post_install", "-at_install")
class TestRequestContext(HamsHttpCase):
    def test_01_get_request_context(self):
        # [@ANCHOR: test_cf_get_request_context]
        # Tests [@ANCHOR: cf_get_request_context]
        """Verify that Cloudflare headers are correctly parsed into the request context."""

        headers = {
            'CF-Connecting-IP': '1.2.3.4',
            'CF-IPCountry': 'US',
            'CF-IPCity': 'New York',
            'CF-IPLongitude': '-74.006',
            'CF-IPLatitude': '40.7128',
            'CF-Threat-Score': '10',
        }

        mock_request_obj = MagicMock()
        mock_request_obj.httprequest.headers = headers
        mock_request_obj.httprequest.remote_addr = '1.1.1.1'

        with mock_request(self.env):
            # safe_patch replaces the target, so we mock the entire object
            # Alternatively, we patch the object directly.
            mock_obj = self.safe_patch('odoo.addons.cloudflare.models.edge_context.request')
            mock_obj.httprequest.headers = headers
            mock_obj.httprequest.remote_addr = '1.1.1.1'

            context = self.env['cloudflare.utils'].get_request_context()

            self.assertEqual(context['ip'], '1.2.3.4')
            self.assertEqual(context['country'], 'US')
            self.assertEqual(context['city'], 'New York')
            self.assertEqual(context['threat_score'], '10')

        # # Verified by [@ANCHOR: test_cf_get_request_context]

    def test_02_get_request_context_no_headers(self):
        """Verify fallback when Cloudflare headers are missing."""
        with mock_request(self.env):
            mock_obj = self.safe_patch('odoo.addons.cloudflare.models.edge_context.request')
            mock_obj.httprequest.headers = {}
            mock_obj.httprequest.remote_addr = '1.1.1.1'

            context = self.env['cloudflare.utils'].get_request_context()
            self.assertEqual(context['ip'], '1.1.1.1')
            self.assertIsNone(context['country'])
