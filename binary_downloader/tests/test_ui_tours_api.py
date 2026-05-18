# -*- coding: utf-8 -*-
import os
from odoo import http, tools
from odoo.http import request
from odoo.tests import HttpCase, tagged
from unittest.mock import patch
class BinaryDownloaderTestController(http.Controller):
    @http.route("/test/dummy_bin", type="http", auth="none", csrf=False)
    def download_dummy_bin(self, **kwargs):
        headers = [("Content-Type", "application/octet-stream"), ("ETag", '"dummy-etag-1234"')]
        return request.make_response(b"1234", headers=headers)
@tagged("post_install", "-at_install")
class TestBinaryDownloaderTour(HttpCase):
    # [@ANCHOR: test_binary_install_tour]
    def setUp(self):
        super().setUp()
        self.env.ref("base.user_admin").lang = "en_US"
    def tearDown(self):
        super().tearDown()
        data_dir = tools.config.get("data_dir", "/var/lib/odoo")
        test_bin_path = os.path.join(data_dir, "hams_bin", "tourbin")
        if os.path.exists(test_bin_path):
            try: os.remove(test_bin_path)
            except OSError: pass
    def get_tourbin_path(self, *args, **kwargs):
        data_dir = tools.config.get("data_dir", "/var/lib/odoo")
        return os.path.join(data_dir, "hams_bin", "tourbin")
    @patch("odoo.addons.binary_downloader.models.binary_manifest.BinaryManifest.ensure_executable", side_effect=get_tourbin_path)
    # Tested by [@ANCHOR: test_binary_install_tour]
    def test_binary_install_tour(self, mock_ensure):
        self.start_tour("/odoo", "binary_install_tour", login="admin")
