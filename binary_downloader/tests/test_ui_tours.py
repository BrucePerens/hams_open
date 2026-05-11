# -*- coding: utf-8 -*-
import os
from odoo import http
from odoo.http import request
from odoo.tests import HttpCase, tagged

class BinaryDownloaderTestController(http.Controller):
    @http.route('/test/dummy_bin', type='http', auth='public')
    def download_dummy_bin(self, **kwargs):
        # Serves a basic 4-byte payload (b"1234") for the UI tour to download
        # The SHA256 hash for "1234" is 03ac674216f3e15c761ee1a5e255f067953623c8b388b4459e13f978d7c846f4
        return request.make_response(b"1234")

@tagged("post_install", "-at_install")
class TestBinaryDownloaderTour(HttpCase):
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
            except OSError:
                pass

    def test_binary_install_tour(self):
        self.start_tour("/web?debug=1", "binary_install_tour", login="admin")
