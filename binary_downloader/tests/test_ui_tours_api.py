# -*- coding: utf-8 -*-
# Part of Odoo. See LICENSE file for full copyright and licensing details.
#
# This file is part of the HAMS project and is licensed under the AGPL-3.0 license.
# See the LICENSE file in the project root for full license information.
import os
import logging
from odoo import http
from odoo.http import request
from odoo.addons.zero_sudo.tests.common import HamsHttpCase
from odoo.tests import tagged

_logger = logging.getLogger(__name__)


class BinaryDownloaderTestController(http.Controller):
    @http.route("/test/dummy_bin", type="http", auth="none", csrf=False)
    def download_dummy_bin(self, **kwargs):
        # Serves a basic 4-byte payload (b"1234") for the UI tour to download
        # The SHA256 hash for "1234" is 03ac674216f3e15c761ee1a5e255f067953623c8b388b4459e13f978d7c846f4
        headers = [
            ("Content-Type", "application/octet-stream"),
            ("ETag", '"dummy-etag-1234"'),
        ]
        return request.make_response(b"1234", headers=headers)


@tagged("post_install", "-at_install")
class TestBinaryDownloaderTour(HamsHttpCase):
    # [@ANCHOR: test_binary_install_tour]
    def setUp(self):
        super().setUp()
        # Force the admin user to use a deterministic US English locale
        # to prevent headless browser translation crashes during UI tours.
        self.env.ref("base.user_admin").lang = "en_US"

    def tearDown(self):
        super().tearDown()
        # Clean up the physical dummy binary created by the tour
        test_bin_path = "/var/lib/odoo/hams_bin/tourbin"
        if os.path.exists(test_bin_path):
            try:
                os.remove(test_bin_path)
            except OSError as e:
                _logger.warning("Could not remove tour binary: %s", e)

    def test_binary_install_tour(self):
        # [!] DIAGNOSTIC FOR AI: Running UI tour for binary installation.
        # Tests [@ANCHOR: UX_BINARY_INSTALL]
        self.safe_patch(
            "odoo.addons.binary_downloader.models.binary_manifest.BinaryManifest.ensure_executable",
            return_value="/var/lib/odoo/hams_bin/tourbin",
        )
        self.start_tour(
            "/odoo?debug=1&action=binary_downloader.action_binary_downloader_manifest",
            "binary_install_tour",
            login="admin",
        )
