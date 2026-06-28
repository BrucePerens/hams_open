# -*- coding: utf-8 -*-
import io
import tarfile
import hashlib
import os
import stat
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
        with tarfile.open(  # audit-ignore-path
            fileobj=tar_stream, mode="w:gz"
        ) as tar:  # audit-ignore-path-traversal
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
