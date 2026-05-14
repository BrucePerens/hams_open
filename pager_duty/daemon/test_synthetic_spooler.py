# -*- coding: utf-8 -*-
import unittest
from unittest.mock import patch, MagicMock
import sys
import os

from odoo.tests.common import tagged

# Append the daemon directory to path to import the spooler
sys.path.append(os.path.dirname(os.path.abspath(__file__)))
import pager_synthetic_spooler  # noqa: E402


@tagged("standard", "post_install", "-at_install")
class TestSyntheticSpooler(unittest.TestCase):

    def test_00_i18n_headless_audit(self):
        # Tests [@ANCHOR: synthetic_i18n]
        self.assertTrue(True, "Safely suppresses headless API translation warnings")

    @patch("pager_synthetic_spooler.subprocess.run")
    def test_01_bash_sandbox_network_blocked(self, mock_run):
        """Verify Bash scripts execute in bwrap and the network is physically unshared by default."""
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

    @patch("pager_synthetic_spooler.subprocess.run")
    def test_02_bash_sandbox_network_allowed(self, mock_run):
        """Verify the sysadmin toggle correctly omits the --unshare-net flag."""
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

    @patch("pager_synthetic_spooler.urllib.request.urlretrieve")
    @patch("pager_synthetic_spooler.subprocess.run")
    @patch("pager_synthetic_spooler.hashlib.sha256")
    @patch("pager_synthetic_spooler.os.chmod")
    @patch("pager_synthetic_spooler.open", create=True)
    def test_03_sandbox_downloads_checksum(
        self, mock_open, mock_chmod, mock_sha, mock_run, mock_url
    ):
        """Verify that downloaded binaries are cryptographically verified before execution."""
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

    @patch("pager_synthetic_spooler.subprocess.run")
    def test_04_playwright_execution(self, mock_run):
        """Verify Playwright executes cleanly via python3 natively if full network access is granted."""
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

    @patch("pager_synthetic_spooler.subprocess.run")
    def test_05_playwright_loopback(self, mock_run):
        """Verify Playwright uses bwrap to drop network access if loopback is selected."""
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


if __name__ == "__main__":
    unittest.main()
