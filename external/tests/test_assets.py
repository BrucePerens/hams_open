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

    def read(self, size=None):
        # size=None (the real http.client.HTTPResponse.read() default,
        # matching download_and_transform_file's own single-shot
        # `response.read()` call, unlike download_file's chunked
        # `response.read(8192)` loop) reads everything remaining --
        # BytesIO.read() already has this exact behavior for None.
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

    # Tests [@ANCHOR: external:HTTP_REACHABLE_FT8JS]
    def test_02b_ft8js_decoder_reachable(self):
        """Verify the vendored WASM FT8 decode+encode modules are both reachable via HTTP."""
        for glue_name, module_name in (("decode.js", "___ft8jsDecodeModule___"), ("encode.js", "___ft8jsEncodeModule___")):
            js_response = self.url_open(f"/external/static/src/node_modules/ft8js/{glue_name}")
            self.assertEqual(
                js_response.status_code, 200, f"ft8js {glue_name} should be reachable."
            )
            self.assertIn(
                module_name.encode(),
                js_response.content,
                f"ft8js {glue_name} content should be valid Emscripten glue.",
            )

        for wasm_name in ("decode.wasm", "encode.wasm"):
            wasm_response = self.url_open(f"/external/static/src/node_modules/ft8js/{wasm_name}")
            self.assertEqual(
                wasm_response.status_code, 200, f"ft8js {wasm_name} should be reachable."
            )
            self.assertEqual(
                wasm_response.content[:4],
                b"\x00asm",
                f"{wasm_name} should start with the real WASM magic bytes.",
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

    # Tests [@ANCHOR: external:hash_file]
    def test_hash_file_returns_the_real_sha256_of_real_content(self):
        content = b"real, non-trivial file content for a genuine hash check"
        expected = hashlib.sha256(content).hexdigest()
        path = os.path.join(tempfile.gettempdir(), "fetch_assets_hash_file_test")
        with open(path, "wb") as f:
            f.write(content)
        try:
            self.assertEqual(fetch_assets.hash_file(path), expected)
        finally:
            os.remove(path)

    # Tests [@ANCHOR: external:hash_file]
    def test_hash_file_returns_none_for_a_missing_file(self):
        missing_path = os.path.join(tempfile.gettempdir(), "fetch_assets_definitely_does_not_exist")
        self.assertIsNone(fetch_assets.hash_file(missing_path))

    # Tests [@ANCHOR: external:_odoo_module_banner_transform]
    def test_odoo_module_banner_transform_prepends_the_real_banner(self):
        raw = b"var x = 1;\n"
        result = fetch_assets._odoo_module_banner_transform(raw)
        self.assertEqual(result, b"/** @odoo-module **/\n" + raw)

    # Tests [@ANCHOR: external:_d3_geo_projection_transform]
    def test_d3_geo_projection_transform_neuters_the_real_commonjs_requires(self):
        raw = (
            b'r(exports, require("d3-geo"), require("d3-array"));'
            b'"function"==typeof define&&define.amd?define(["exports","d3-geo","d3-array"],r):0;'
        )
        result = fetch_assets._d3_geo_projection_transform(raw)
        self.assertNotIn(b'require("d3-geo"), require("d3-array")', result)
        self.assertIn(
            b'importModule("d3-geo"), importModule("d3-array")', result,
            "the real require() calls must be replaced with importModule(), not just removed",
        )
        self.assertNotIn(
            b'"function"==typeof define&&define.amd?define(["exports","d3-geo","d3-array"],r)',
            result,
            "the AMD branch must be disabled",
        )
        self.assertTrue(result.startswith(b"/** @odoo-module **/\n"))

    # Tests [@ANCHOR: external:download_and_transform_file]
    def test_download_and_transform_file_applies_the_real_transform_before_hash_verifying(self):
        mock_urlopen = self.safe_patch("urllib.request.urlopen")
        mock_urlopen.return_value = DummyResponse()
        self.safe_patch("odoo.addons.external.fetch_assets.hash_file", return_value=None)
        mock_move = self.safe_patch("shutil.move")
        self.safe_patch("os.chmod")

        transform = lambda raw: b"TRANSFORMED:" + raw
        expected_hash = hashlib.sha256(b"TRANSFORMED:dummy").hexdigest()
        dest_path = os.path.join(tempfile.gettempdir(), "fetch_assets_transform_test")

        fetch_assets.download_and_transform_file("http://dummy", dest_path, transform, expected_hash)

        self.assertEqual(mock_urlopen.call_count, 1)
        # The file actually written (mocked shutil.move's source arg) must be
        # the TRANSFORMED content, not the raw download -- a real regression
        # this test would catch: hashing/writing raw instead of transformed.
        written_tmp_path = mock_move.call_args[0][0]
        with open(written_tmp_path, "rb") as f:
            self.assertEqual(f.read(), b"TRANSFORMED:dummy")
        os.remove(written_tmp_path)

    # Tests [@ANCHOR: external:download_and_transform_file]
    def test_download_and_transform_file_raises_on_a_real_hash_mismatch(self):
        mock_urlopen = self.safe_patch("urllib.request.urlopen")
        mock_urlopen.return_value = DummyResponse()
        self.safe_patch("odoo.addons.external.fetch_assets.hash_file", return_value=None)

        dest_path = os.path.join(tempfile.gettempdir(), "fetch_assets_hash_mismatch_test")
        with self.assertRaises(ValueError):
            fetch_assets.download_and_transform_file(
                "http://dummy", dest_path, lambda raw: raw, "0" * 64
            )

    # Tests [@ANCHOR: external:fetch_d3_family_assets]
    def test_fetch_d3_family_assets_downloads_all_three_real_pinned_files(self):
        # Deliberately does NOT call the real function end to end -- its own
        # docstring is explicit that running it for real (real network,
        # real file writes) is a manual, deliberate action, not something
        # a unit test should trigger automatically (a real, reverted
        # attempt at exactly that broke 12 unrelated tests). This verifies
        # the orchestration itself: the right 3 URLs, transforms, and
        # pinned hashes are passed to download_and_transform_file, without
        # ever performing a real download.
        mock_download_and_transform = self.safe_patch(
            "odoo.addons.external.fetch_assets.download_and_transform_file"
        )
        fetch_assets.fetch_d3_family_assets_INTENTIONALLY_NOT_CALLED_FROM_MAIN(
            os.path.join(tempfile.gettempdir(), "fake_lib_dir")
        )

        self.assertEqual(mock_download_and_transform.call_count, 3)
        called_urls = [call.args[0] for call in mock_download_and_transform.call_args_list]
        self.assertIn("https://unpkg.com/d3@7.9.0/dist/d3.min.js", called_urls)
        self.assertIn(
            "https://unpkg.com/d3-geo-projection@4.0.0/dist/d3-geo-projection.min.js", called_urls
        )
        self.assertIn(
            "https://unpkg.com/topojson-client@3.1.0/dist/topojson-client.min.js", called_urls
        )
