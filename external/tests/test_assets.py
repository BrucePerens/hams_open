# This software is distributed under the terms of the Affero General Public License (AGPL-3).

# -*- coding: utf-8 -*-
# Copyright © Bruce Perens K6BP. Proprietary, Trade-Secret.

import os
import hashlib
import tempfile
from io import BytesIO

from odoo.addons.zero_sudo.tests.common import HamsHttpCase
from odoo.tests import tagged
from odoo.tools import mute_logger

from odoo.addons.external import fetch_assets

class DummyResponse:
    def __init__(self):
        self.content = BytesIO(b"dummy")

    def read(self, size):
        return self.content.read(size)

    def __enter__(self):
        return self

    def __exit__(self, exc_type, exc_val, exc_tb):
        pass


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

    # Tests [@ANCHOR: external:HTTP_NO_HEAD]
    def test_03_no_head_request(self):
        """Verify fetch_assets does not use HEAD requests."""
        mock_urlopen = self.safe_patch("urllib.request.urlopen")
        mock_urlopen.return_value = DummyResponse()
        
        self.safe_patch("odoo.addons.external.fetch_assets.hash_file", return_value=None)
        self.safe_patch("shutil.move")
        self.safe_patch("os.chmod")
        self.safe_patch("os.remove")
        
        dummy_hash = hashlib.sha256(b"dummy").hexdigest()
        dummy_path = os.path.join(tempfile.gettempdir(), "dummy_file_assets_test")
        
        fetch_assets.download_file("http://dummy", dummy_path, dummy_hash)
        
        self.assertEqual(mock_urlopen.call_count, 1)

    # Tests [@ANCHOR: external:HTTP_NO_MASKING]
    def test_04_exception_masking(self):
        """Verify exception is not masked if tmp_path does not exist."""
        mock_urlopen = self.safe_patch("urllib.request.urlopen")
        mock_urlopen.return_value = DummyResponse()
        
        self.safe_patch("odoo.addons.external.fetch_assets.hash_file", return_value=None)
        mock_move = self.safe_patch("shutil.move")
        mock_move.side_effect = Exception("Original Exception")
        
        self.safe_patch("os.path.exists", return_value=False)
        mock_remove = self.safe_patch("os.remove")
        mock_remove.side_effect = FileNotFoundError("Should not be called")
        
        dummy_hash = hashlib.sha256(b"dummy").hexdigest()
        dummy_path = os.path.join(tempfile.gettempdir(), "dummy_file_assets_test2")
        
        with self.assertRaisesRegex(Exception, "Original Exception"), mute_logger('odoo.addons.external.fetch_assets'):
            fetch_assets.download_file("http://dummy", dummy_path, dummy_hash)

    # Tests [@ANCHOR: external:TRANSFORMERS_MIN]
    def test_05_transformers_min_js(self):
        """Verify transformers_url uses minified JS."""
        mock_download = self.safe_patch("odoo.addons.external.fetch_assets.download_file")
        fetch_assets.main()
        
        transformers_call = None
        for call in mock_download.call_args_list:
            args, kwargs = call
            if "transformers" in args[0]:
                transformers_call = args
                
        self.assertIsNotNone(transformers_call)
        self.assertTrue(transformers_call[0].endswith("transformers.min.js"), "URL should be minified")
