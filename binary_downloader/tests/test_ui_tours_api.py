# -*- coding: utf-8 -*-
# Part of Odoo. See LICENSE file for full copyright and licensing details.
#
# This file is part of the HAMS project and is licensed under the AGPL-3.0 license.
# See the LICENSE file in the project root for full license information.
import os
import logging

from odoo.addons.zero_sudo.tests.common import HamsHttpCase
from odoo.tests import tagged

_logger = logging.getLogger(__name__)


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
            os.remove(test_bin_path)

    def test_binary_install_tour(self):
        # [!] DIAGNOSTIC FOR AI: Running UI tour for binary installation.
        # Tests [@ANCHOR: UX_BINARY_INSTALL]
        url_action = "/odoo?debug=1&action=binary_downloader.action_binary_downloader_manifest"
        self.start_tour(url_action, "binary_install_tour", login="admin")
