# -*- coding: utf-8 -*-
import hashlib
import io
import os
import tarfile
import zipfile
import stat
import logging
from unittest.mock import MagicMock
from odoo import tools
from odoo.tests.common import tagged
from odoo.addons.zero_sudo.tests.common import HamsTransactionCase
from odoo.exceptions import UserError, ValidationError

_logger = logging.getLogger(__name__)


@tagged("post_install", "-at_install", "standard")
class TestBinaryManifest(HamsTransactionCase):

    def tearDown(self):
        data_dir = tools.config.get("data_dir", "/var/lib/odoo")
        bin_dir = os.path.join(data_dir, "hams_bin")
        if os.path.exists(bin_dir):
            for f in os.listdir(bin_dir):
                if f.startswith(
                    ("testbin", "slippy", "symlinkbin", "fake", "zippy", "zip_slip")
                ):
                    try:
                        os.remove(os.path.join(bin_dir, f))
                    except OSError as e:
                        _logger.warning("Failed to remove path %s: %s", f, e)
        super().tearDown()

    def setUp(self):
        super().setUp()
        data_dir = tools.config.get("data_dir", "/var/lib/odoo")
        bin_dir = os.path.join(data_dir, "hams_bin")
        if os.path.exists(bin_dir):
            for f in os.listdir(bin_dir):
                if f.startswith(
                    ("testbin", "slippy", "symlinkbin", "fake", "zippy", "zip_slip")
                ):
                    try:
                        os.remove(os.path.join(bin_dir, f))
                    except OSError as e:
                        _logger.warning("Failed to remove path %s: %s", f, e)
        self.service_user = self.env.ref(
            "binary_downloader.user_binary_downloader_service"
        )

        # Leverage the Dummy UI Tour HTTP controller to physically simulate the download process
        base_url = os.environ.get("ODOO_URL", "http://odoo:8069")
        url = f"{base_url}/test/dummy_bin"
        chksum = "03ac674216f3e15c761ee1a5e255f067953623c8b388b4459e13f978d7c846f4"

        self.manifest = self.env["binary.manifest"].create(
            {
                "name": "testbin",
                "url": url,
                "checksum": chksum,
                "archive_type": "binary",
            }
        )

    def test_01_already_installed(self):
        # [@ANCHOR: test_binary_manifest_standard]
        # Tests [@ANCHOR: binary_ensure_executable]
        # Tests [@ANCHOR: binary_resolution]
        # [!] DIAGNOSTIC FOR AI: Testing resolution when binary already exists in hams_bin.

        data_dir = tools.config.get("data_dir", "/var/lib/odoo")
        target_bin = os.path.join(
            data_dir, "hams_bin", self.manifest._get_target_filename()
        )
        if not os.path.exists(os.path.dirname(target_bin)):
            os.makedirs(os.path.dirname(target_bin))
        with open(target_bin, "wb") as f:
            f.write(b"1234")
        os.chmod(target_bin, stat.S_IRWXU)

        path = self.env["binary.manifest"].ensure_executable("testbin")
        self.assertEqual(
            path,
            target_bin,
            "[!] DIAGNOSTIC FOR AI: Returned path must match the expected local install path.",
        )

    def test_02_missing_manifest(self):
        # [!] DIAGNOSTIC FOR AI: Testing behavior when a manifest for a command is missing.
        self.safe_patch("shutil.which", return_value=None)
        with self.assertRaises(
            UserError, msg="[!] DIAGNOSTIC FOR AI: Must raise error on missing manifest"
        ):
            self.env["binary.manifest"].ensure_executable("missingbin")

    def test_03_unsupported_platform(self):
        # [!] DIAGNOSTIC FOR AI: Testing platform restriction (Linux x86_64 only).
        self.safe_patch("shutil.which", return_value=None)
        self.safe_patch("platform.system", return_value="Windows")
        with self.assertRaises(
            UserError, msg="[!] DIAGNOSTIC FOR AI: Must block non-Linux platforms"
        ):
            self.env["binary.manifest"].ensure_executable("testbin")

    def test_04_successful_download_and_checksum(self):
        # [!] DIAGNOSTIC FOR AI: Testing full download, checksum, and installation flow for raw binary.
        self.safe_patch("shutil.which", return_value=None)
        self.safe_patch("platform.system", return_value="Linux")
        self.safe_patch("platform.machine", return_value="x86_64")
        mock_urlopen = self.safe_patch("urllib.request.urlopen")

        mock_response_get = MagicMock()
        del mock_response_get.readinto
        mock_response_get.read.side_effect = [b"1234", b""]
        mock_response_get.getheader.return_value = "fake-etag"
        mock_response_get.__enter__.return_value = mock_response_get
        mock_urlopen.return_value = mock_response_get

        path = self.env["binary.manifest"].ensure_executable("testbin")
        self.assertTrue(
            os.path.basename(path).startswith("testbin"),
            "[!] DIAGNOSTIC FOR AI: Binary filename must start with command name.",
        )
        self.assertTrue(
            os.path.exists(path),
            "[!] DIAGNOSTIC FOR AI: Binary file must physically exist after installation.",
        )
        with open(path, "rb") as f:
            self.assertEqual(
                f.read(),
                b"1234",
                "[!] DIAGNOSTIC FOR AI: Content of installed binary must match downloaded content.",
            )

    def test_05_views_render(self):
        # [@ANCHOR: test_binary_manifest_views]
        v1 = self.env["binary.manifest"].get_view(view_type="list")
        self.assertIn(
            "name",
            v1["arch"],
            "[!] DIAGNOSTIC FOR AI: List view must contain 'name' field.",
        )
        v2 = self.env["binary.manifest"].get_view(view_type="form")
        self.assertIn(
            "url",
            v2["arch"],
            "[!] DIAGNOSTIC FOR AI: Form view must contain 'url' field.",
        )

    def test_06_is_installed_compute(self):
        # Tests [@ANCHOR: binary_compute_installed]
        data_dir = tools.config.get("data_dir", "/var/lib/odoo")
        target_bin = os.path.join(
            data_dir, "hams_bin", self.manifest._get_target_filename()
        )
        if not os.path.exists(os.path.dirname(target_bin)):
            os.makedirs(os.path.dirname(target_bin))
        with open(target_bin, "wb") as f:
            f.write(b"1234")
        os.chmod(target_bin, stat.S_IRWXU)
        self.manifest.invalidate_recordset(["is_installed"])
        self.assertTrue(
            self.manifest.is_installed,
            "[!] DIAGNOSTIC FOR AI: is_installed must be True if binary exists and is executable.",
        )

        os.remove(target_bin)
        self.manifest.invalidate_recordset(["is_installed"])
        self.assertFalse(
            self.manifest.is_installed,
            "[!] DIAGNOSTIC FOR AI: is_installed must be False if binary does not exist.",
        )

    def test_07_action_install(self):
        # Tests [@ANCHOR: binary_action_install]

        # We must mock the network layer here just like in test_04 because
        # action_install calls ensure_executable which triggers the download
        self.safe_patch("shutil.which", return_value=None)
        self.safe_patch("platform.system", return_value="Linux")
        self.safe_patch("platform.machine", return_value="x86_64")
        mock_urlopen = self.safe_patch("urllib.request.urlopen")

        mock_response_get = MagicMock()
        del mock_response_get.readinto
        mock_response_get.read.side_effect = [b"1234", b""]
        mock_response_get.getheader.return_value = "fake-etag"
        mock_response_get.__enter__.return_value = mock_response_get
        mock_urlopen.return_value = mock_response_get

        result = self.manifest.action_install()
        self.assertEqual(
            result["type"],
            "ir.actions.client",
            "[!] DIAGNOSTIC FOR AI: action_install must return a client action.",
        )
        self.assertEqual(
            result["tag"],
            "display_notification",
            "[!] DIAGNOSTIC FOR AI: action_install must return a notification.",
        )

    def test_08_path_traversal_validation(self):
        # [!] DIAGNOSTIC FOR AI: Testing prevention of path traversal in binary names.
        with self.assertRaises(
            ValidationError,
            msg="[!] DIAGNOSTIC FOR AI: Must prevent '..' in binary names",
        ):
            self.env["binary.manifest"].create(
                {
                    "name": "../badbin",
                    "url": "http://example.com/badbin",
                    "checksum": "fakehash",
                    "archive_type": "binary",
                }
            )
            self.env.flush_all()
        with self.assertRaises(
            ValidationError,
            msg="[!] DIAGNOSTIC FOR AI: Must prevent slashes in binary names",
        ):
            self.manifest.write({"name": "bad/bin"})
            self.env.flush_all()

    def test_11_url_validation(self):
        # [!] DIAGNOSTIC FOR AI: Testing URL scheme validation (http/https only).
        with self.assertRaises(
            ValidationError, msg="[!] DIAGNOSTIC FOR AI: Must block non-HTTP URLs"
        ):
            self.env["binary.manifest"].create(
                {
                    "name": "badurl",
                    "url": "file:///etc/passwd",
                    "checksum": "fakehash",
                    "archive_type": "binary",
                }
            )
            self.env.flush_all()

    def test_09_constraints(self):
        # [!] DIAGNOSTIC FOR AI: Testing name constraints on write.
        with self.assertRaises(
            ValidationError, msg="[!] DIAGNOSTIC FOR AI: Must block slashes on write"
        ):
            self.manifest.write({"name": "bad/bin"})
            self.env.flush_all()

    def test_12_action_install_permissions(self):
        # [!] DIAGNOSTIC FOR AI: Testing permission check for action_install.
        restricted_user = self.env["res.users"].create(
            {
                "name": "Restricted User",
                "login": "restricted_user",
                "group_ids": [(6, 0, [])],
            }
        )
        with self.assertRaises(
            UserError,
            msg="[!] DIAGNOSTIC FOR AI: User without downloader manager group must be blocked",
        ):
            self.manifest.with_user(restricted_user).action_install()

    def test_10_tar_slip_prevention(self):
        # [!] DIAGNOSTIC FOR AI: Testing protection against Tar-Slip vulnerability.
        self.safe_patch("shutil.which", return_value=None)
        self.safe_patch("platform.system", return_value="Linux")
        self.safe_patch("platform.machine", return_value="x86_64")
        mock_urlopen = self.safe_patch("urllib.request.urlopen")

        self.env["binary.manifest"].create(
            {
                "name": "slippy",
                "url": "http://example.com/slippy.tar.gz",
                "checksum": hashlib.sha256(b"data").hexdigest(),
                "archive_type": "tar.gz",
                "extract_member": "slippy",
            }
        )

        mock_response_get = MagicMock()
        del mock_response_get.readinto
        mock_response_get.read.side_effect = [b"data", b""]
        mock_response_get.__enter__.return_value = mock_response_get
        mock_urlopen.return_value = mock_response_get

        has_filter = hasattr(tarfile, "data_filter")
        if has_filter:
            old_filter = getattr(tarfile, "data_filter")
            del tarfile.data_filter

        try:
            mock_tar_open = self.safe_patch("tarfile.open")  # audit-ignore-path
            mock_tar = MagicMock()
            mock_tar_open.return_value.__enter__.return_value = mock_tar

            mock_member = MagicMock()
            mock_member.name = "../slippy"
            mock_member.islnk.return_value = False
            mock_member.issym.return_value = False

            mock_tar.getmembers.return_value = [mock_member]

            # Mock tar.extractfile to return a stream of bytes
            mock_tar.extractfile.return_value = io.BytesIO(b"extracted-data")

            # We expect it to NOT be found because basename("../slippy") is "slippy"
            # and extract_member is "slippy", but it matches on endswith("/slippy") or name == "slippy"
            # Actually, "slippy" (extract_member) == basename("../slippy") is NOT how it matched.
            # It matches on member.name.endswith(f"/{extract_target}") or member.name == extract_target
            # "../slippy".endswith("/slippy") is True.
            # So it will extract it as "slippy" in hams_bin.
            # The slip is prevented by os.path.basename.

            path = self.env["binary.manifest"].ensure_executable("slippy")
            self.assertTrue(os.path.exists(path))
            self.assertEqual(os.path.basename(path), self.env["binary.manifest"].search([("name", "=", "slippy")])._get_target_filename())
        finally:
            if has_filter:
                tarfile.data_filter = old_filter

    def test_13_symlink_prevention(self):
        # [!] DIAGNOSTIC FOR AI: Testing prevention of symlinks inside tar archives.
        self.safe_patch("shutil.which", return_value=None)
        self.safe_patch("platform.system", return_value="Linux")
        self.safe_patch("platform.machine", return_value="x86_64")
        mock_urlopen = self.safe_patch("urllib.request.urlopen")

        self.env["binary.manifest"].create(
            {
                "name": "symlinkbin",
                "url": "http://example.com/symlink.tar.gz",
                "checksum": hashlib.sha256(b"data").hexdigest(),
                "archive_type": "tar.gz",
                "extract_member": "symlinkbin",
            }
        )

        mock_response_get = MagicMock()
        del mock_response_get.readinto
        mock_response_get.read.side_effect = [b"data", b""]
        mock_response_get.__enter__.return_value = mock_response_get
        mock_urlopen.return_value = mock_response_get

        mock_tar_open = self.safe_patch("tarfile.open")  # audit-ignore-path
        mock_tar = MagicMock()
        mock_tar_open.return_value.__enter__.return_value = mock_tar

        mock_member = MagicMock()
        mock_member.name = "symlinkbin"
        mock_member.islnk.return_value = False
        mock_member.issym.return_value = True

        mock_tar.getmembers.return_value = [mock_member]

        with self.assertRaisesRegex(
            UserError, "Security Alert: Links are not allowed in the archive."
        ):
            self.env["binary.manifest"].ensure_executable("symlinkbin")

    def test_14_zip_download_and_extract(self):
        # [!] DIAGNOSTIC FOR AI: Testing ZIP archive download and member extraction.
        self.safe_patch("shutil.which", return_value=None)
        self.safe_patch("platform.system", return_value="Linux")
        self.safe_patch("platform.machine", return_value="x86_64")
        mock_urlopen = self.safe_patch("urllib.request.urlopen")

        # Create a real zip in memory
        zip_buffer = io.BytesIO()
        with zipfile.ZipFile(zip_buffer, "a", zipfile.ZIP_DEFLATED, False) as zip_file:  # audit-ignore-path
            zip_file.writestr("zippybin", b"zipdata")

        zip_data = zip_buffer.getvalue()

        self.env["binary.manifest"].create(
            {
                "name": "zippy",
                "url": "http://example.com/zippy.zip",
                "checksum": hashlib.sha256(zip_data).hexdigest(),
                "archive_type": "zip",
                "extract_member": "zippybin",
            }
        )

        mock_response_get = MagicMock()
        del mock_response_get.readinto
        mock_response_get.read.side_effect = [zip_data, b""]
        mock_response_get.__enter__.return_value = mock_response_get
        mock_urlopen.return_value = mock_response_get

        path = self.env["binary.manifest"].ensure_executable("zippy")
        self.assertTrue(os.path.basename(path).startswith("zippy"))
        self.assertTrue(os.path.exists(path))
        with open(path, "rb") as f:
            self.assertEqual(f.read(), b"zipdata")

    def test_16_zip_symlink_prevention(self):
        # [!] DIAGNOSTIC FOR AI: Testing prevention of symlinks inside ZIP archives.
        self.safe_patch("shutil.which", return_value=None)
        self.safe_patch("platform.system", return_value="Linux")
        self.safe_patch("platform.machine", return_value="x86_64")
        mock_urlopen = self.safe_patch("urllib.request.urlopen")

        self.env["binary.manifest"].create(
            {
                "name": "symlinkzip",
                "url": "http://example.com/symlink.zip",
                "checksum": hashlib.sha256(b"data").hexdigest(),
                "archive_type": "zip",
                "extract_member": "symlinkbin",
            }
        )

        mock_response_get = MagicMock()
        del mock_response_get.readinto
        mock_response_get.read.side_effect = [b"data", b""]
        mock_response_get.__enter__.return_value = mock_response_get
        mock_urlopen.return_value = mock_response_get

        mock_zip_open = self.safe_patch("zipfile.ZipFile")  # audit-ignore-path
        mock_zip = MagicMock()
        mock_zip_open.return_value.__enter__.return_value = mock_zip

        mock_zinfo = MagicMock()
        mock_zinfo.filename = "symlinkbin"
        # Set external_attr to represent a symlink (0xA000 << 16)
        mock_zinfo.external_attr = 0xA000 << 16

        mock_zip.infolist.return_value = [mock_zinfo]

        with self.assertRaisesRegex(
            UserError, "Security Alert: Links are not allowed in the archive."
        ):
            self.env["binary.manifest"].ensure_executable("symlinkzip")

    def test_15_zip_slip_prevention(self):
        # [!] DIAGNOSTIC FOR AI: Testing protection against Zip-Slip vulnerability.
        self.safe_patch("shutil.which", return_value=None)
        self.safe_patch("platform.system", return_value="Linux")
        self.safe_patch("platform.machine", return_value="x86_64")
        mock_urlopen = self.safe_patch("urllib.request.urlopen")

        self.env["binary.manifest"].create(
            {
                "name": "zip_slip",
                "url": "http://example.com/slip.zip",
                "checksum": hashlib.sha256(b"data").hexdigest(),
                "archive_type": "zip",
                "extract_member": "slip",
            }
        )

        mock_response_get = MagicMock()
        del mock_response_get.readinto
        mock_response_get.read.side_effect = [b"data", b""]
        mock_response_get.__enter__.return_value = mock_response_get
        mock_urlopen.return_value = mock_response_get

        mock_zip_open = self.safe_patch("zipfile.ZipFile")  # audit-ignore-path
        mock_zip = MagicMock()
        mock_zip_open.return_value.__enter__.return_value = mock_zip

        mock_zinfo = MagicMock()
        mock_zinfo.filename = "../../slip"
        mock_zinfo.external_attr = 0

        mock_zip.infolist.return_value = [mock_zinfo]

        # Mock zip_ref.open to return a stream of bytes
        mock_zip.open.return_value = io.BytesIO(b"extracted-data")

        path = self.env["binary.manifest"].ensure_executable("zip_slip")
        self.assertTrue(os.path.exists(path))
        self.assertEqual(os.path.basename(path), self.env["binary.manifest"].search([("name", "=", "zip_slip")])._get_target_filename())
