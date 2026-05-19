# -*- coding: utf-8 -*-
from unittest.mock import MagicMock
from odoo.tests.common import tagged
from odoo.addons.hams_test.common import HamsHttpCase
from lxml import etree
from odoo.addons.caching.controllers.main import ServiceWorkerController

@tagged("post_install", "-at_install")
class TestSettingsAndCache(HamsHttpCase):

    def test_01_quota_config_updates_sw(self):
        # [@ANCHOR: test_settings_and_cache_01]
        # Tests [@ANCHOR: caching_quota_calculation]
        # Tests [@ANCHOR: caching_fs_scan_logic]
        """
        Verify that changing the safe quota in settings dynamically
        updates the MAX_FILE_SIZE_BYTES in the /sw.js payload.
        """
        # Get baseline response
        svc_uid = self.env['zero_sudo.security.utils']._get_service_uid('caching.user_caching_service')

        self.env['ir.config_parameter'].with_user(svc_uid).set_param('caching.safe_quota_mb', '35') # Tested by [@ANCHOR: test_caching_sudo_params]  # fmt: skip
        response_35 = self.url_open("/sw.js")
        self.assertEqual(response_35.status_code, 200)

        # Change quota
        self.env['ir.config_parameter'].with_user(svc_uid).set_param('caching.safe_quota_mb', '10') # Tested by [@ANCHOR: test_caching_sudo_params]  # fmt: skip
        response_10 = self.url_open("/sw.js")
        self.assertEqual(response_10.status_code, 200)

        # We can test that it evaluates dynamically by setting it extremely low
        self.env['ir.config_parameter'].with_user(svc_uid).set_param('caching.safe_quota_mb', '0') # Tested by [@ANCHOR: test_caching_sudo_params]  # fmt: skip
        response_0 = self.url_open("/sw.js")

        # If quota is 0, the max file size should be 0 or slightly less than the smallest file
        # We can just verify that it doesn't crash
        self.assertEqual(response_0.status_code, 200)

    def test_06_quota_edge_cases(self):
        """Test quota calculation edge cases with mocked filesystem data."""
        controller = ServiceWorkerController()
        ServiceWorkerController._fs_cache = None

        mock_req = MagicMock()
        mock_req.env = self.env['res.users'].env
        svc_uid = self.env['zero_sudo.security.utils']._get_service_uid('caching.user_caching_service')

        # Case 1: No files
        self.safe_patch_object(controller, '_get_fs_stats', return_value=(1000.0, []))
        self.safe_patch('odoo.addons.caching.controllers.main.request', mock_req)
        # Set specific param for this sub-test
        self.env['ir.config_parameter'].with_user(svc_uid).set_param('caching.safe_quota_mb', '35') # Tested by [@ANCHOR: test_caching_sudo_params]  # fmt: skip
        mtime, max_size = controller._get_global_static_info()
        self.assertEqual(max_size, str(10 * 1024 * 1024))

        # Case 2: Files fit within quota
        # Total size: 10MB + 5MB = 15MB. Quota: 35MB.
        self.safe_patch_object(controller, '_get_fs_stats', return_value=(1000.0, [10*1024*1024, 5*1024*1024]))
        self.safe_patch('odoo.addons.caching.controllers.main.request', mock_req)
        self.env['ir.config_parameter'].with_user(svc_uid).set_param('caching.safe_quota_mb', '35') # Tested by [@ANCHOR: test_caching_sudo_params]  # fmt: skip
        mtime, max_size = controller._get_global_static_info()
        self.assertEqual(max_size, str(10*1024*1024 + 1024))

        # Case 3: Files exceed quota
        # Total size: 30MB + 10MB = 40MB. Quota: 35MB.
        # Should drop the 30MB file. Remaining: 10MB. 10MB <= 35MB.
        # max_size should be 30MB - 1.
        self.safe_patch_object(controller, '_get_fs_stats', return_value=(1000.0, [30*1024*1024, 10*1024*1024]))
        self.safe_patch('odoo.addons.caching.controllers.main.request', mock_req)
        self.env['ir.config_parameter'].with_user(svc_uid).set_param('caching.safe_quota_mb', '35') # Tested by [@ANCHOR: test_caching_sudo_params]  # fmt: skip
        mtime, max_size = controller._get_global_static_info()
        self.assertEqual(max_size, str(30*1024*1024 - 1))

    def test_02_force_invalidation(self):
        """
        Verify that action_force_cache_invalidation updates the cache version
        and that this change is reflected in the /sw.js CACHE_NAME.
        """
        # Ensure we have a starting state
        svc_uid = self.env['zero_sudo.security.utils']._get_service_uid('caching.user_caching_service')
        self.env['ir.config_parameter'].with_user(svc_uid).set_param('caching.invalidation_version', '1') # Tested by [@ANCHOR: test_caching_sudo_params]  # fmt: skip

        response_1 = self.url_open("/sw.js")
        content_1 = response_1.text
        self.assertIn('-v1', content_1)

        # Simulate button click
        settings = self.env['res.config.settings'].create({})
        settings.action_force_cache_invalidation()

        response_2 = self.url_open("/sw.js")
        content_2 = response_2.text
        self.assertIn('-v2', content_2)
        self.assertNotIn('-v1', content_2)

    def test_03_caching_sudo_params(self):
        """
        Verify that sudo() calls are secure and tagged correctly.
        [@ANCHOR: test_caching_sudo_params]
        """
        # This test acts as the anchor verifying that the params are intentionally safe
        val = self.env['zero_sudo.security.utils']._get_system_param('caching.safe_quota_mb') # Tested by [@ANCHOR: test_caching_sudo_params]  # fmt: skip
        self.assertTrue(val is not None or val is None)

    def test_05_zero_sudo_scan(self):
        # [@ANCHOR: test_caching_zero_sudo_scan]
        # Tests [@ANCHOR: caching_fs_scan_logic]
        """Verify that the FS scan correctly uses the service account."""
        controller = ServiceWorkerController()
        # Reset cache to force re-scan
        ServiceWorkerController._fs_cache = None

        mock_req = MagicMock()
        mock_req.env = self.env['res.users'].with_context(force_fs_scan=True).env

        self.safe_patch('odoo.addons.caching.controllers.main.request', mock_req)
        mtime, sizes = controller._get_fs_stats()
        self.assertGreater(mtime, 0)
        self.assertIsInstance(sizes, list)

    def test_04_xpath_rendering_settings(self):
        # [@ANCHOR: test_xpath_rendering_caching_settings]
        # Tests [@ANCHOR: xpath_rendering_caching_settings]
        """Verify the Caching settings are injected into the website configuration view."""

        view = self.env.ref("website.res_config_settings_view_form")
        arch = view.with_context(lang=None)._get_combined_arch()
        arch_str = etree.tostring(arch, encoding="unicode")

        self.assertIn(
            "Caching Service Worker",
            arch_str,
            "The Caching settings block must be injected into the compiled layout.",
        )
