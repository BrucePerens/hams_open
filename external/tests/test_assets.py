# -*- coding: utf-8 -*-
# Copyright © Bruce Perens K6BP. Proprietary, Trade-Secret.

from odoo.addons.zero_sudo.tests.common import HamsHttpCase
from odoo.tests import tagged


@tagged("post_install", "-at_install", "external")
class TestExternalAssets(HamsHttpCase):
    # Tests [@ANCHOR: external:HTTP_REACHABLE_LEAFLET]
    def test_01_leaflet_assets_reachable(self):
        """Verify Leaflet JS and CSS are reachable via HTTP."""
        js_url = "/external/static/src/node_modules/leaflet/leaflet.js"
        css_url = "/external/static/src/node_modules/leaflet/leaflet.css"

        js_response = self.url_open(js_url)
        self.assertEqual(
            js_response.status_code, 200, "Leaflet JS should be reachable."
        )
        self.assertIn(
            b"Leaflet", js_response.content, "Leaflet JS content should be valid."
        )

        css_response = self.url_open(css_url)
        self.assertEqual(
            css_response.status_code, 200, "Leaflet CSS should be reachable."
        )
        self.assertIn(
            b".leaflet-container",
            css_response.content,
            "Leaflet CSS content should be valid.",
        )

    # Tests [@ANCHOR: external:HTTP_REACHABLE_TRANSFORMERS]
    def test_02_transformers_assets_reachable(self):
        """Verify Transformers JS is reachable via HTTP."""
        js_url = "/external/static/src/node_modules/transformers/transformers.js"

        js_response = self.url_open(js_url)
        self.assertEqual(
            js_response.status_code, 200, "Transformers JS should be reachable."
        )
        self.assertIn(
            b"transformers",
            js_response.content,
            "Transformers JS content should be valid.",
        )
