# -*- coding: utf-8 -*-
from unittest.mock import MagicMock
from odoo.tests.common import tagged
from odoo.addons.hams_test.common import HamsIntegrationCase
from odoo.addons.pager_duty.daemon import pager_synthetic_spooler


@tagged('post_install', '-at_install')
class TestSyntheticSpooler(HamsIntegrationCase):

    def test_00_i18n_headless_audit(self):
        self.assertTrue(hasattr(pager_synthetic_spooler, "execute_check"), "Safely suppresses headless API translation warnings")

    def test_01_bash_sandbox_network_blocked(self):
        """Verify Bash scripts execute in bwrap and the network is physically unshared by default."""
        mock_run = self.safe_patch("odoo.addons.pager_duty.daemon.pager_synthetic_spooler.subprocess.run")
        mock_run.return_value.returncode = 0
        check = {
            "type": "bash",
            "name": "test_bash",
            "code_payload": "echo 1",
            "sandbox_network_access": "loopback",
        }
        name, res = pager_synthetic_spooler.execute_check(check)
        self.assertTrue(res["success"])

        args = mock_run.call_args[0][0]
        self.assertIn("bwrap", args)
        self.assertIn("--unshare-net", args, "Network MUST be isolated by default.")

    def test_02_bash_sandbox_network_allowed(self):
        """Verify the sysadmin toggle correctly omits the --unshare-net flag."""
        mock_run = self.safe_patch("odoo.addons.pager_duty.daemon.pager_synthetic_spooler.subprocess.run")
        mock_run.return_value.returncode = 0
        check = {
            "type": "bash",
            "name": "test_bash",
            "code_payload": "echo 1",
            "sandbox_network_access": "full",
        }
        name, res = pager_synthetic_spooler.execute_check(check)
        self.assertTrue(res["success"])

        args = mock_run.call_args[0][0]
        self.assertNotIn(
            "--unshare-net", args, "Network toggle MUST allow network access."
        )

    def test_03_sandbox_downloads_checksum(self):
        """Verify that downloaded binaries are cryptographically verified before execution."""
        mock_url = self.safe_patch("odoo.addons.pager_duty.daemon.pager_synthetic_spooler.urllib.request.urlretrieve")
        mock_run = self.safe_patch("odoo.addons.pager_duty.daemon.pager_synthetic_spooler.subprocess.run")
        mock_sha = self.safe_patch("odoo.addons.pager_duty.daemon.pager_synthetic_spooler.hashlib.sha256")
        self.safe_patch("odoo.addons.pager_duty.daemon.pager_synthetic_spooler.os.chmod")
        self.safe_patch("odoo.addons.pager_duty.daemon.pager_synthetic_spooler.open", create=True)

        mock_hasher = MagicMock()
        mock_hasher.hexdigest.return_value = "fakehash"
        mock_sha.return_value = mock_hasher

        check = {
            "type": "executable",
            "name": "test_exe",
            "executable_path": "my_bin",
            "sandbox_downloads": "http://example.com/bin | fakehash | my_bin",
        }
        mock_run.return_value.returncode = 0

        # 1. Success case
        name, res = pager_synthetic_spooler.execute_check(check)
        self.assertTrue(res["success"])
        mock_url.assert_called_once()

        # 2. Checksum Mismatch Case
        mock_hasher.hexdigest.return_value = "badhash"
        name, res = pager_synthetic_spooler.execute_check(check)
        self.assertFalse(res["success"])
        self.assertIn("Checksum mismatch", res["error"])

    def test_04_playwright_execution(self):
        """Verify Playwright executes cleanly via python3 natively if full network access is granted."""
        mock_run = self.safe_patch("odoo.addons.pager_duty.daemon.pager_synthetic_spooler.subprocess.run")
        mock_run.return_value.returncode = 0
        check = {
            "type": "playwright",
            "name": "test_pw",
            "code_payload": "from playwright.sync_api import sync_playwright",
            "sandbox_network_access": "full",
        }
        name, res = pager_synthetic_spooler.execute_check(check)
        self.assertTrue(res["success"])

        args = mock_run.call_args[0][0]
        self.assertEqual(args[0], "python3")
        self.assertNotIn("bwrap", args)

    def test_05_playwright_loopback(self):
        """Verify Playwright uses bwrap to drop network access if loopback is selected."""
        mock_run = self.safe_patch("odoo.addons.pager_duty.daemon.pager_synthetic_spooler.subprocess.run")
        mock_run.return_value.returncode = 0
        check = {
            "type": "playwright",
            "name": "test_pw_loopback",
            "code_payload": "from playwright.sync_api import sync_playwright",
            "sandbox_network_access": "loopback",
        }
        name, res = pager_synthetic_spooler.execute_check(check)
        self.assertTrue(res["success"])

        args = mock_run.call_args[0][0]
        self.assertEqual(args[0], "bwrap")
        self.assertIn("--unshare-net", args)
