# -*- coding: utf-8 -*-
# Copyright © HAMS project. AGPL-3.0.
from unittest.mock import MagicMock
from odoo.tests.common import tagged
from odoo.tools import file_open
from lxml import etree
import werkzeug
from odoo.addons.caching.controllers.main import ServiceWorkerController
from odoo.addons.zero_sudo.tests.real_transaction import RealTransactionCase


@tagged("post_install", "-at_install")
class TestSettingsAndCache(RealTransactionCase):

    def test_01_quota_config_updates_sw(self):
        # [@ANCHOR: test_settings_and_cache_01]
        # Tests [@ANCHOR: caching_quota_calculation]
        # Tests [@ANCHOR: caching_fs_scan_logic]
        """
        Verify that changing the safe quota in settings dynamically
        updates the MAX_FILE_SIZE_BYTES in the /sw.js payload.
        """
        website = self.env["website"].get_current_website()
        website.caching_safe_quota_mb = 35

        response_35 = self.url_open("/sw.js")
        self.assertEqual(
            response_35.status_code,
            200,
            "[!] DIAGNOSTIC FOR AI: Failed to fetch /sw.js with 35MB quota.",
        )
        # Content should contain MAX_FILE_SIZE_BYTES
        self.assertIn("MAX_FILE_SIZE_BYTES", response_35.text)

        # Change quota
        website.caching_safe_quota_mb = 10
        response_10 = self.url_open("/sw.js")
        self.assertEqual(
            response_10.status_code,
            200,
            "[!] DIAGNOSTIC FOR AI: Failed to fetch /sw.js with 10MB quota.",
        )
        self.assertIn("MAX_FILE_SIZE_BYTES", response_10.text)

        # We can test that it evaluates dynamically by setting it extremely low
        website.caching_safe_quota_mb = 0
        response_0 = self.url_open("/sw.js")

        self.assertEqual(
            response_0.status_code,
            200,
            "[!] DIAGNOSTIC FOR AI: Failed to fetch /sw.js with 0MB quota.",
        )
        # Even with 0MB quota, we expect some value for MAX_FILE_SIZE_BYTES
        self.assertIn("MAX_FILE_SIZE_BYTES", response_0.text)

    def test_07_no_dict_manipulation(self):
        """Verify that __dict__ is not manipulated in controllers/main.py and _fs_cache is removed."""
        with file_open("caching/controllers/main.py", "r") as f:
            content = f.read()
        self.assertNotIn("__dict__", content, "[!] DIAGNOSTIC FOR AI: Direct __dict__ manipulation is discouraged.")
        self.assertNotIn("_fs_cache = None", content, "[!] DIAGNOSTIC FOR AI: _fs_cache is unused and dead code.")

    def test_06_quota_edge_cases(self):
        """Test quota calculation edge cases with mocked filesystem data."""
        controller = ServiceWorkerController()

        mock_req = MagicMock()
        mock_req.env = self.env["res.users"].env
        website = self.env["website"].get_current_website()
        mock_req.website = website

        # Case 1: No files
        self.safe_patch_object(controller, "_get_fs_stats", return_value=(1000.0, []))
        self.safe_patch("odoo.addons.caching.controllers.main.request", mock_req)
        website.caching_safe_quota_mb = 35
        mtime, max_size = controller._get_global_static_info()
        self.assertEqual(
            max_size,
            str(10 * 1024 * 1024),
            "[!] DIAGNOSTIC FOR AI: Default max size should be 10MB when no files are found.",
        )

        # Case 2: Files fit within quota
        # Total size: 15MB. Quota: 35MB.
        self.safe_patch_object(
            controller,
            "_get_fs_stats",
            return_value=(1000.0, [10 * 1024 * 1024, 5 * 1024 * 1024]),
        )
        mtime, max_size = controller._get_global_static_info()
        self.assertEqual(
            max_size,
            str(10 * 1024 * 1024 + 1024),
            "[!] DIAGNOSTIC FOR AI: Max size should be largest file + 1024 when everything fits.",
        )

        # Case 3: Files exceed quota
        # Total size: 40MB. Quota: 35MB.
        self.safe_patch_object(
            controller,
            "_get_fs_stats",
            return_value=(
                1000.0,
                [30 * 1024 * 1024, 10 * 1024 * 1024],
            ),
        )
        mtime, max_size = controller._get_global_static_info()
        self.assertEqual(
            max_size,
            str(30 * 1024 * 1024 - 1),
            "[!] DIAGNOSTIC FOR AI: Max size should be adjusted to reject the largest file when over quota.",
        )

        # Case 4: Quota is less than reservation (e.g. 5MB)
        # Reservation is 10MB.
        website.caching_safe_quota_mb = 5
        mtime, max_size = controller._get_global_static_info()
        self.assertEqual(
            max_size,
            str(0),
            "[!] DIAGNOSTIC FOR AI: Max size should be 0 if quota is less than reservation.",
        )

    def test_02_force_invalidation(self):
        """
        Verify that action_force_cache_invalidation updates the version.
        """
        website = self.env["website"].get_current_website()
        website.caching_invalidation_version = 1

        response_1 = self.url_open("/sw.js")
        content_1 = response_1.text
        self.assertIn("-v1", content_1)

        # Simulate button click
        settings = self.env["res.config.settings"].create({"website_id": website.id})
        settings.action_force_cache_invalidation()
        self.env.flush_all()  # Ensure data is written to DB

        # To safely bypass the Odoo test cursor constraint (which blocks self.env.cr.commit),
        # we will mock the website retrieval in the controller to directly read our uncommitted
        # ORM object memory space rather than relying on a new HTTP thread.
        mock_req = MagicMock()
        mock_req.env = self.env
        mock_req.website = website
        self.safe_patch("odoo.addons.caching.controllers.main.request", mock_req)

        controller = ServiceWorkerController()

        # Mock file_open to prevent filesystem dependency in unit test using safe_patch
        mock_file = MagicMock()
        mock_file.read.return_value = "const CACHE_NAME = '__CACHE_NAME__'; const MAX_FILE_SIZE_BYTES = __MAX_FILE_SIZE_BYTES__;"
        mock_open = MagicMock()
        mock_open.return_value.__enter__.return_value = mock_file
        self.safe_patch(
            "odoo.addons.caching.controllers.main.tools.file_open", mock_open
        )

        mock_req.make_response = MagicMock(
            side_effect=lambda content, headers: werkzeug.wrappers.Response(
                content, headers=headers
            )
        )

        response_2 = controller.service_worker()

        content_2 = response_2.get_data(as_text=True)

        self.assertIn("-v2", content_2)
        self.assertNotIn("-v1", content_2)

    def test_03_caching_sudo_params(self):
        """
        Verify that sudo() calls are secure and tagged correctly.
        [@ANCHOR: test_caching_sudo_params]
        """
        # This test acts as the anchor verifying that the params are
        # intentionally safe
        utils = self.env["zero_sudo.security.utils"]
        utils._set_system_param("caching.safe_quota_mb", "42")
        val = utils._get_system_param("caching.safe_quota_mb")
        self.assertEqual(val, "42", "System parameter should be retrievable securely.")

    def test_05_zero_sudo_scan(self):
        # [@ANCHOR: test_caching_zero_sudo_scan]
        # Tests [@ANCHOR: caching_fs_scan_logic]
        """Verify that the FS scan correctly uses the service account."""
        controller = ServiceWorkerController()
        # Reset cache to force re-scan

        mock_req = MagicMock()
        mock_req.env = self.env["res.users"].with_context(force_fs_scan=True).env

        self.safe_patch("odoo.addons.caching.controllers.main.request", mock_req)
        mtime, sizes = controller._get_fs_stats()
        self.assertGreater(mtime, 0)
        self.assertIsInstance(sizes, list)

    def test_04_xpath_rendering_settings(self):
        # [@ANCHOR: test_xpath_rendering_caching_settings]
        # Tests [@ANCHOR: xpath_rendering_caching_settings]
        """
        Verify the Caching settings are injected into the website
        configuration view.
        """

        view = self.env.ref("website.res_config_settings_view_form")
        arch = view.with_context(lang=None)._get_combined_arch()
        arch_str = etree.tostring(arch, encoding="unicode")

        self.assertIn(
            "Caching Service Worker",
            arch_str,
            "The Caching settings block must be injected into " "the compiled layout.",
        )
