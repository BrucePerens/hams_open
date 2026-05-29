# -*- coding: utf-8 -*-
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
        # Physical integration test for kopia installation
        # This will actually download the real kopia binary from GitHub as configured in data
        self.env.ref("binary_downloader.binary_manifest_kopia")

        # Ensure it's not on system PATH to force local install
        self.safe_patch("shutil.which", return_value=None)

        path = self.env["binary.manifest"].ensure_executable("kopia")

        self.assertEqual(path, self.test_bin)
        self.assertTrue(os.path.exists(path))
        self.assertTrue(os.access(path, os.X_OK))

        # Verify it's actually an executable by checking its mode
        mode = os.stat(path).st_mode
        self.assertTrue(bool(mode & stat.S_IXUSR))
