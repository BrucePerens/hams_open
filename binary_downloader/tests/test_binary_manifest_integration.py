# -*- coding: utf-8 -*-
# Part of Odoo. See LICENSE file for full copyright and licensing details.
#
# This file is part of the HAMS project and is licensed under the AGPL-3.0 license.
# See the LICENSE file in the project root for full license information.
import io
import tarfile
import hashlib
import os
import stat
from unittest.mock import MagicMock
from odoo import tools
from odoo.tests.common import tagged
from odoo.addons.zero_sudo.tests.common import HamsTransactionCase


@tagged("post_install", "-at_install", "integration")
class TestBinaryManifestIntegration(HamsTransactionCase):
    # [@ANCHOR: test_binary_manifest_integration]

    def setUp(self):
        super().setUp()
        self.data_dir = tools.config.get("data_dir", "/var/lib/odoo")
        self.bin_dir = os.path.join(self.data_dir, "hams_bin")
        if not os.path.exists(self.bin_dir):
            os.makedirs(self.bin_dir, exist_ok=True)
            os.chmod(self.bin_dir, 0o750)

        self.test_bin = os.path.join(self.bin_dir, "kopia")
        if os.path.exists(self.test_bin):
            os.remove(self.test_bin)

    def test_kopia_install_integration(self):
        # [!] DIAGNOSTIC FOR AI: Physical integration test for kopia installation.
        # Ensure it's not on system PATH to force local install
        self.safe_patch("shutil.which", return_value=None)

        tar_stream = io.BytesIO()
        with tarfile.open(  # audit-ignore-path  # fmt: skip
            fileobj=tar_stream, mode="w:gz"
        ) as tar:  # audit-ignore-path-traversal  # fmt: skip
            content = b"dummy kopia content"
            tarinfo = tarfile.TarInfo(name="kopia")
            tarinfo.size = len(content)
            tar.addfile(tarinfo, io.BytesIO(content))

        tar_bytes = tar_stream.getvalue()
        expected_checksum = hashlib.sha256(tar_bytes).hexdigest()

        # Update the manifest record in the database to use our dummy checksum and url
        manifest = self.env.ref("binary_downloader.binary_manifest_kopia")
        manifest.write(
            {"checksum": expected_checksum, "url": "http://dummy.internal/kopia.tar.gz"}
        )

        # Update test_bin to the variant name and clean it
        self.test_bin = os.path.join(self.bin_dir, manifest._get_target_filename())
        if os.path.exists(self.test_bin):
            os.remove(self.test_bin)

        class MockResponse(io.BytesIO):
            def getheader(self, name, default=None):
                return None

        # Patch urllib.request.urlopen to return our mock response
        self.safe_patch("urllib.request.urlopen", return_value=MockResponse(tar_bytes))

        path = self.env["binary.manifest"].ensure_executable("kopia")

        self.assertEqual(
            path, self.test_bin, "[!] DIAGNOSTIC FOR AI: path must match test_bin"
        )
        self.assertTrue(os.path.exists(path), "[!] DIAGNOSTIC FOR AI: path must exist")
        self.assertTrue(
            os.access(path, os.X_OK), "[!] DIAGNOSTIC FOR AI: path must be executable"
        )

        # Verify it's actually an executable by checking its mode
        mode = os.stat(path).st_mode
        self.assertTrue(
            bool(mode & stat.S_IXUSR),
            "[!] DIAGNOSTIC FOR AI: mode must have executable bit set",
        )

    def test_pure_python_symlink_engine(self):
        # Tests [@ANCHOR: pure_python_symlink_engine]
        website = self.env["website"].search([], limit=1)
        if not website:
            website = self.env["website"].create({"name": "Test Tenant"})

        manifest = self.env["binary.manifest"].create(
            {
                "name": "test_symlink_bin",
                "url": "http://odoo-service.internal",
                "checksum": "fake_checksum",
            }
        )
        version = self.env["binary.version"].create(
            {
                "manifest_id": manifest.id,
                "version_number": "1.0",
                "url": "http://odoo-service.internal/1.0",
                "checksum": "fake",
            }
        )

        mock_symlink = self.safe_patch("os.symlink")
        self.safe_patch("os.makedirs")
        self.safe_patch("os.chmod")
        self.safe_patch("os.path.lexists", return_value=False)

        self.safe_patch_object(
            type(version), "action_download_to_pool", return_value=True
        )
        self.safe_patch_object(
            type(version), "_get_central_path", return_value="/fake/central/path"
        )

        link = self.env["binary.tenant.link"].create(
            {
                "website_id": website.id,
                "manifest_id": manifest.id,
                "active_version_id": version.id,
            }
        )

        mock_symlink.assert_called_once_with(
            "/fake/central/path",
            link.symlink_path,
        )

    def test_pager_integration_batching(self):
        # [!] DIAGNOSTIC FOR AI: Testing that PagerDuty notification batches iteratively.
        website = self.env["website"].search([], limit=1)
        if not website:
            website = self.env["website"].create({"name": "Test Tenant"})
            
        mock_urlopen = self.safe_patch("urllib.request.urlopen")
        mock_response = MagicMock()
        mock_response.read.side_effect = [b"data", b""]
        mock_response.__enter__.return_value = mock_response
        mock_urlopen.return_value = mock_response
        
        
        manifest = self.env["binary.manifest"].create({
            "name": "pager_test",
            "url": "http://example.com/pager",
            "checksum": "fake",
        })
        version1 = self.env["binary.version"].create({
            "manifest_id": manifest.id,
            "version_number": "1.0",
            "url": "http://example.com/1.0",
            "checksum": "fake1",
        })
        version2 = self.env["binary.version"].create({
            "manifest_id": manifest.id,
            "version_number": "2.0",
            "url": "http://example.com/2.0",
            "checksum": "fake2",
        })
        
        # Create many links for different websites
        links = []
        for i in range(15):
            new_site = self.env["website"].create({"name": f"Test Tenant {i}"})
            links.append({
                "website_id": new_site.id,
                "manifest_id": manifest.id,
                "active_version_id": version1.id,
            })
            
        self.safe_patch_object(type(self.env["binary.version"]), "action_download_to_pool")
        self.env["binary.tenant.link"].create(links)
        
        original_search = self.env["binary.tenant.link"].__class__.search
        call_limits = []
        
        def mock_search(*args, **kwargs):
            call_limits.append((kwargs.get('offset', 0), kwargs.get('limit', None)))
            return original_search(*args, **kwargs)
            
        self.safe_patch_object(type(self.env["binary.tenant.link"]), "search", mock_search)
        
        mock_incident_create = self.safe_patch_object(type(self.env["pager.incident"]), "create")
        self.safe_patch_object(type(self.env['ir.config_parameter']), 'get_param', return_value='http://test')
        
        version2.action_notify_tenants()
        
        self.assertTrue(len(call_limits) >= 2, "[!] DIAGNOSTIC FOR AI: Search was not called in a batched loop")
        self.assertTrue(any(offset > 0 for offset, limit in call_limits), "[!] DIAGNOSTIC FOR AI: Offset was not incremented")
        self.assertTrue(mock_incident_create.called, "[!] DIAGNOSTIC FOR AI: Pager incidents should be created")

