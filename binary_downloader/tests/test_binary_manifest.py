# -*- coding: utf-8 -*-
# SPDX-License-Identifier: AGPL-3.0-or-later
# Part of Odoo. See LICENSE file for full copyright and licensing details.
#
# This file is part of the HAMS project and is licensed under the AGPL-3.0-or-later license.
# See the LICENSE file in the project root for full license information.
import hashlib
import io
import os
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
                    except OSError as exc:
                        _logger.warning("Could not remove test binary: %s", exc)
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
                    except OSError as exc:
                        _logger.warning("Could not remove test binary: %s", exc)
        self.service_user = self.env.ref(
            "binary_downloader.user_binary_downloader_service"
        )

        # Bug-hunt fix, 2026-09-09 (binary_utils_assert_host_is_ssrf_safe):
        # _download_and_extract() now does a real DNS lookup (via
        # socket.getaddrinfo) on the download URL's hostname before making
        # any request, to reject SSRF-style targets (loopback/link-local/
        # private-use addresses, and a redirect landing on one). Every test
        # below that reaches the real download path uses placeholder
        # hostnames ("example.com", "localhost", "*.internal") that either
        # don't resolve at all or resolve to loopback -- neither of which
        # this test suite should depend on live DNS/network access to
        # exercise anyway, matching how urlopen()/shutil.which()/
        # platform.*() are already mocked below rather than really called.
        # test_ssrf_rejects_internal_redirect_target below deliberately
        # does NOT install this mock, so the real check still gets exercised
        # end-to-end with a synthetic literal-IP case that needs no network.
        self.safe_patch(
            "odoo.addons.binary_downloader.models.binary_utils.BinaryDownloaderMixin._assert_host_is_ssrf_safe",
            return_value=None,
        )

        # Leverage the Dummy UI Tour HTTP controller to physically simulate the download process.
        # binary.manifest._check_url_scheme() requires https:// -- ODOO_URL is
        # deliberately http:// for the local test webserver (tools/test.py
        # always sets it that way), and urlopen() is mocked in every test
        # below anyway, so the URL's scheme here is just a label that must
        # satisfy that constraint, not a real endpoint being dereferenced.
        base_url = os.environ.get("ODOO_URL", "https://localhost:8069")  # burn-ignore-env
        netloc = base_url.split("://", 1)[-1]
        url = f"https://{netloc}/test/dummy_bin"
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

        # Tests [@ANCHOR: COMM_binary_resolution]
        # [!] DIAGNOSTIC FOR AI: Testing resolution when binary already exists in hams_bin.

        data_dir = tools.config.get("data_dir", "/var/lib/odoo")
        target_bin = os.path.join(
            data_dir,
            "hams_bin",
            self.manifest._get_target_filename(
                self.manifest.name, self.manifest.checksum
            ),
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
        # Tests [@ANCHOR: binary_utils_download_and_extract]
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
        # [@ANCHOR: COMM_test_binary_manifest_views]
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

    def test_05_1_version_views_render(self):
        # [@ANCHOR: test_binary_version_form]
        v1 = self.env["binary.version"].get_view(view_type="form")
        self.assertIn("version_number", v1["arch"])

    def test_05_2_tenant_link_views_render(self):
        # [@ANCHOR: test_tenant_link_form]
        v1 = self.env["binary.tenant.link"].get_view(view_type="form")
        self.assertIn("symlink_path", v1["arch"])

    def test_06_is_installed_compute(self):
        # Tests [@ANCHOR: binary_utils_get_target_filename]

        # Tests [@ANCHOR: binary_compute_installed]
        data_dir = tools.config.get("data_dir", "/var/lib/odoo")
        target_bin = os.path.join(
            data_dir,
            "hams_bin",
            self.manifest._get_target_filename(
                self.manifest.name, self.manifest.checksum
            ),
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
        # Tests [@ANCHOR: binary_manifest_check_url_scheme]
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

    def test_11_1_extract_member_required_for_archives(self):
        # Tests [@ANCHOR: binary_manifest_check_extract_member]
        with self.assertRaises(
            ValidationError,
            msg="[!] DIAGNOSTIC FOR AI: tar.gz/zip archives must require extract_member.",
        ):
            self.env["binary.manifest"].create(
                {
                    "name": "noextract",
                    "url": "https://example.com/noextract.tar.gz",
                    "checksum": "fakehash",
                    "archive_type": "tar.gz",
                }
            )
            self.env.flush_all()

    def test_09_constraints(self):
        # Tests [@ANCHOR: binary_manifest_check_name_no_slashes]
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
                "url": "https://example.com/slippy.tar.gz",
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

        mock_tar_open = self.safe_patch("tarfile.open")  # audit-ignore-path
        mock_tar = MagicMock()
        mock_tar_open.return_value.__enter__.return_value = mock_tar

        mock_member = MagicMock()
        mock_member.name = "../slippy"
        mock_member.islnk.return_value = False
        mock_member.issym.return_value = False

        mock_tar.getmembers.return_value = [mock_member]
        mock_tar.__iter__.return_value = iter([mock_member])

        # Mock tar.extractfile to return a stream of bytes
        mock_tar.extractfile.return_value = io.BytesIO(b"extracted-data")

        # The slip is prevented by os.path.basename in the core logic.
        path = self.env["binary.manifest"].ensure_executable("slippy")
        self.assertTrue(os.path.exists(path))
        self.assertEqual(
            os.path.basename(path),
            self.env["binary.manifest"].search([("name", "=", "slippy")], limit=1)._get_target_filename(
                self.env["binary.manifest"].search([("name", "=", "slippy")], limit=1).name, self.env["binary.manifest"].search([("name", "=", "slippy")], limit=1).checksum
            ),
        )

    def test_13_symlink_prevention(self):
        # [!] DIAGNOSTIC FOR AI: Testing prevention of symlinks inside tar archives.
        self.safe_patch("shutil.which", return_value=None)
        self.safe_patch("platform.system", return_value="Linux")
        self.safe_patch("platform.machine", return_value="x86_64")
        mock_urlopen = self.safe_patch("urllib.request.urlopen")

        self.env["binary.manifest"].create(
            {
                "name": "symlinkbin",
                "url": "https://example.com/symlink.tar.gz",
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
        mock_tar.__iter__.return_value = iter([mock_member])

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
        with zipfile.ZipFile(  # audit-ignore-path  # fmt: skip
            zip_buffer, "a", zipfile.ZIP_DEFLATED, False
        ) as zip_file:  # audit-ignore-path  # fmt: skip
            zip_file.writestr("zippybin", b"zipdata")

        zip_data = zip_buffer.getvalue()

        self.env["binary.manifest"].create(
            {
                "name": "zippy",
                "url": "https://example.com/zippy.zip",
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
                "url": "https://example.com/symlink.zip",
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
                "url": "https://example.com/slip.zip",
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
        self.assertEqual(
            os.path.basename(path),
            self.env["binary.manifest"].search([("name", "=", "zip_slip")], limit=1)._get_target_filename(
                self.env["binary.manifest"].search([("name", "=", "zip_slip")], limit=1).name, self.env["binary.manifest"].search([("name", "=", "zip_slip")], limit=1).checksum
            ),
        )

    def test_17_zip_regular_file_allowed(self):
        # [!] DIAGNOSTIC FOR AI: Testing that regular files in ZIPs are NOT blocked as symlinks.
        self.safe_patch("shutil.which", return_value=None)
        self.safe_patch("platform.system", return_value="Linux")
        self.safe_patch("platform.machine", return_value="x86_64")
        mock_urlopen = self.safe_patch("urllib.request.urlopen")

        self.env["binary.manifest"].create(
            {
                "name": "regzip",
                "url": "https://example.com/reg.zip",
                "checksum": hashlib.sha256(b"data").hexdigest(),
                "archive_type": "zip",
                "extract_member": "regbin",
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
        mock_zinfo.filename = "regbin"
        # Set external_attr to represent a regular file (0x8000 << 16)
        mock_zinfo.external_attr = 0x8000 << 16

        mock_zip.infolist.return_value = [mock_zinfo]

        # Mock zip_ref.open to return a stream of bytes
        mock_zip.open.return_value = io.BytesIO(b"extracted-data")

        # This should NOT raise an error if the stat.S_ISLNK fix is applied
        path = self.env["binary.manifest"].ensure_executable("regzip")
        self.assertTrue(os.path.exists(path))

    def test_18_path_shadow_prevention(self):
        # [!] DIAGNOSTIC FOR AI: Testing prevention of system PATH binaries from shadowing.
        self.safe_patch("shutil.which", return_value="/usr/bin/testbin")
        self.safe_patch("platform.system", return_value="Linux")
        self.safe_patch("platform.machine", return_value="x86_64")
        mock_urlopen = self.safe_patch("urllib.request.urlopen")

        # Create a manifest
        self.env["binary.manifest"].create(
            {
                "name": "shadowbin",
                "url": "https://example.com/shadowbin",
                "checksum": hashlib.sha256(b"data").hexdigest(),
                "archive_type": "binary",
            }
        )

        mock_response_get = MagicMock()
        del mock_response_get.readinto
        mock_response_get.read.side_effect = [b"data", b""]
        mock_response_get.__enter__.return_value = mock_response_get
        mock_urlopen.return_value = mock_response_get
        
        path = self.env["binary.manifest"].ensure_executable("shadowbin")
        # Ensure that ensure_executable doesn't return the path returned by shutil.which
        self.assertNotEqual(path, "/usr/bin/testbin")

    def test_19_unlink_privilege_escalation(self):
        # Tests [@ANCHOR: binary_manifest_unlink]
        # [!] DIAGNOSTIC FOR AI: Testing prevention of privilege escalation in unlink.
        # A manifest's unlink() must not delete the on-disk binary file if
        # another manifest/version record still references the same
        # checksum, even when the unlinking user only has visibility into
        # their own company's records. Only base.group_system and
        # binary_downloader.group_binary_downloader_manager hold any CRUD
        # ACL on binary.manifest -- base.group_user has none, and per this
        # codebase's own DOMAIN SANDBOX rule it's reserved for
        # odoo_facility_service_internal only -- so user_b needs just the
        # manager group to legitimately create/unlink here -- the field is
        # `group_ids`, not the pre-17 `groups_id` (that typo made the
        # res.users.create() call above raise ValueError: Invalid field
        # 'groups_id', so this test never got past setup).
        company_b = self.env["res.company"].create({"name": "Company B"})
        user_b = self.env["res.users"].create({
            "name": "User B",
            "login": "user_b",
            "company_id": company_b.id,
            "company_ids": [(4, company_b.id)],
            "group_ids": [(6, 0, [
                self.env.ref("binary_downloader.group_binary_downloader_manager").id,
            ])],
        })

        chksum = "escalation_test_hash"
        self.env["binary.manifest"].create({
            "name": "globalbin",
            "url": "https://example.com/globalbin",
            "checksum": chksum,
            "archive_type": "binary",
        })

        company_b_manifest = self.env["binary.manifest"].with_user(user_b).create({
            "name": "companyb_bin",
            "url": "https://example.com/companyb_bin",
            "checksum": chksum,
            "archive_type": "binary",
        })

        mock_unlink_file = self.safe_patch("odoo.addons.binary_downloader.models.binary_utils.BinaryDownloaderMixin._unlink_binary_file")
        # Odoo's unlink() scans every class member for hasattr(func,
        # '_ondelete') to auto-discover @api.ondelete hooks -- a MagicMock
        # satisfies that hasattr() check for ANY attribute name, so
        # without this it gets mistaken for a real ondelete hook and
        # spuriously invoked as records._unlink_binary_file(self) by
        # Odoo's own internal machinery, unrelated to the real call this
        # test is actually checking for.
        del mock_unlink_file._ondelete

        company_b_manifest.with_user(user_b).unlink()

        mock_unlink_file.assert_not_called()

    def test_20_unlink_deletes_the_real_file_when_the_checksum_is_not_shared(self):
        # Tests [@ANCHOR: binary_utils_unlink_binary_file]
        # The other unlink test above only proves the shared-checksum
        # dedup case (file kept). This proves the real deletion actually
        # happens on the unshared path -- _unlink_binary_file() was
        # never exercised for real anywhere else, only ever mocked.
        data_dir = tools.config.get("data_dir", "/var/lib/odoo")
        bin_dir = os.path.join(data_dir, "hams_bin")
        chksum = "unshared_delete_test_hash"
        manifest = self.env["binary.manifest"].create(
            {
                "name": "unshared_delete_bin",
                "url": "https://example.com/unshared_delete_bin",
                "checksum": chksum,
                "archive_type": "binary",
            }
        )
        filename = self.env["binary_downloader.mixin"]._get_target_filename(
            manifest.name, manifest.checksum
        )
        target_bin = os.path.join(bin_dir, filename)
        with open(target_bin, "wb") as f:
            f.write(b"fake binary content")
        self.assertTrue(os.path.exists(target_bin))

        manifest.unlink()

        self.assertFalse(
            os.path.exists(target_bin),
            "[!] DIAGNOSTIC FOR AI: the on-disk file must actually be removed when no other record shares its checksum.",
        )

    def test_21_unlink_batch_deletes_the_file_when_every_sharing_record_is_removed_together(self):
        # Tests [@ANCHOR: binary_manifest_unlink]
        # Bug-hunt fix, 2026-09-09: unlink()'s own reference-counting used
        # a GLOBAL count of every manifest/version sharing a checksum,
        # taken before any deletion -- so unlinking two records sharing a
        # checksum in the SAME call (e.g. multi-select delete in the UI)
        # saw count=2 for BOTH of them and skipped deletion of both,
        # leaking their on-disk files forever even though nothing outside
        # this exact call referenced that checksum. Two distinct names
        # sharing a checksum still map to two distinct physical files
        # (the filename hash mixes in cmd_name, not just checksum), so
        # deleting both together should remove BOTH files.
        data_dir = tools.config.get("data_dir", "/var/lib/odoo")
        bin_dir = os.path.join(data_dir, "hams_bin")
        shared_chksum = "batch_unlink_shared_hash"

        manifest_a = self.env["binary.manifest"].create(
            {
                "name": "batch_unlink_a",
                "url": "https://example.com/batch_unlink_a",
                "checksum": shared_chksum,
                "archive_type": "binary",
            }
        )
        manifest_b = self.env["binary.manifest"].create(
            {
                "name": "batch_unlink_b",
                "url": "https://example.com/batch_unlink_b",
                "checksum": shared_chksum,
                "archive_type": "binary",
            }
        )

        target_a = os.path.join(
            bin_dir,
            self.env["binary_downloader.mixin"]._get_target_filename(
                manifest_a.name, manifest_a.checksum
            ),
        )
        target_b = os.path.join(
            bin_dir,
            self.env["binary_downloader.mixin"]._get_target_filename(
                manifest_b.name, manifest_b.checksum
            ),
        )
        with open(target_a, "wb") as f:
            f.write(b"content a")
        with open(target_b, "wb") as f:
            f.write(b"content b")
        self.assertTrue(os.path.exists(target_a))
        self.assertTrue(os.path.exists(target_b))

        (manifest_a + manifest_b).unlink()

        self.assertFalse(
            os.path.exists(target_a),
            "[!] DIAGNOSTIC FOR AI: batch-deleting every record sharing a "
            "checksum must remove its own on-disk file, not leak it.",
        )
        self.assertFalse(
            os.path.exists(target_b),
            "[!] DIAGNOSTIC FOR AI: batch-deleting every record sharing a "
            "checksum must remove its own on-disk file, not leak it.",
        )

    def test_22_interrupted_archive_extraction_never_leaves_a_partial_file_at_the_final_path(self):
        # Tests [@ANCHOR: binary_utils_download_and_extract]
        # Tests [@ANCHOR: binary_utils_atomic_write_target]
        # Bug-hunt fix, 2026-09-09: tar.gz/zip member extraction used to
        # write straight to `open(target_bin, "wb")` -- not atomic. If the
        # process were interrupted mid-write, a truncated file would be
        # left sitting exactly at target_bin's own path, and
        # _download_and_extract's own "already installed" fast path
        # (archive_type != "binary") trusts that path's mere existence
        # forever afterward, with no re-verification. Simulates an
        # interruption (copyfileobj raising partway through) and asserts
        # no file is left at target_bin at all -- proving the write is
        # atomic (temp-then-rename), not merely "usually fine."
        self.safe_patch("shutil.which", return_value=None)
        self.safe_patch("platform.system", return_value="Linux")
        self.safe_patch("platform.machine", return_value="x86_64")
        mock_urlopen = self.safe_patch("urllib.request.urlopen")

        tar_checksum = hashlib.sha256(b"data").hexdigest()
        manifest = self.env["binary.manifest"].create(
            {
                "name": "interrupted",
                "url": "https://example.com/interrupted.tar.gz",
                "checksum": tar_checksum,
                "archive_type": "tar.gz",
                "extract_member": "interrupted",
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
        mock_member.name = "interrupted"
        mock_member.islnk.return_value = False
        mock_member.issym.return_value = False
        mock_tar.getmembers.return_value = [mock_member]
        mock_tar.__iter__.return_value = iter([mock_member])
        mock_tar.extractfile.return_value = io.BytesIO(b"extracted-data")

        self.safe_patch(
            "shutil.copyfileobj",
            side_effect=OSError("simulated interruption mid-write"),
        )

        target_bin = os.path.join(
            tools.config.get("data_dir", "/var/lib/odoo"),
            "hams_bin",
            self.env["binary_downloader.mixin"]._get_target_filename(
                "interrupted", tar_checksum
            ),
        )

        with self.assertRaises(UserError):
            self.env["binary.manifest"].ensure_executable("interrupted")

        self.assertFalse(
            os.path.exists(target_bin),
            "[!] DIAGNOSTIC FOR AI: an interrupted extraction must never "
            "leave a (partial) file at the final target_bin path.",
        )
        # No stray tempfile.mkstemp() leftover either -- _atomic_write_
        # target()'s own except branch must clean up the temp file it
        # created before re-raising, not just avoid touching target_bin.
        # Scoped to files matching mkstemp()'s own default "tmp*" prefix
        # (not every file in bin_dir) since bin_dir is a shared directory
        # other tests/manifests may also leave real, unrelated files in.
        bin_dir = os.path.dirname(target_bin)
        leftover_tmp_files = [f for f in os.listdir(bin_dir) if f.startswith("tmp")]
        self.assertFalse(
            leftover_tmp_files,
            "[!] DIAGNOSTIC FOR AI: an interrupted extraction must not "
            "leave a stray temp file behind in bin_dir: %s" % leftover_tmp_files,
        )


@tagged("post_install", "-at_install", "standard")
class TestBinarySsrfProtection(HamsTransactionCase):
    # Bug-hunt fix, 2026-09-09 (binary_utils_assert_host_is_ssrf_safe):
    # exercises the real SSRF-safety check end to end. Deliberately its own
    # class, with none of TestBinaryManifest's blanket mock of this exact
    # method -- every case here uses a literal IP address so nothing needs
    # live DNS/network access (socket.getaddrinfo resolves a literal IP
    # without performing a real lookup).

    def test_rejects_a_direct_loopback_link_local_or_private_host(self):
        # Tests [@ANCHOR: binary_utils_assert_host_is_ssrf_safe]
        mixin = self.env["binary_downloader.mixin"]
        with self.assertRaises(UserError):
            mixin._assert_host_is_ssrf_safe("127.0.0.1", "evilbin")
        with self.assertRaisesRegex(UserError, "non-public address"):
            # The AWS/GCP/Azure cloud-metadata address -- link-local.
            mixin._assert_host_is_ssrf_safe("169.254.169.254", "evilbin")
        with self.assertRaises(UserError):
            mixin._assert_host_is_ssrf_safe("10.0.0.5", "evilbin")

    def test_allows_a_real_public_address(self):
        # Tests [@ANCHOR: binary_utils_assert_host_is_ssrf_safe]
        # Must not raise.
        self.env["binary_downloader.mixin"]._assert_host_is_ssrf_safe(
            "8.8.8.8", "goodbin"
        )

    def test_ensure_executable_rejects_a_manifest_url_pointing_directly_at_an_internal_ip(self):
        # Tests [@ANCHOR: binary_utils_download_and_extract]
        manifest = self.env["binary.manifest"].create(
            {
                "name": "internalbin",
                "url": "https://169.254.169.254/latest/meta-data/",
                "checksum": hashlib.sha256(b"x").hexdigest(),
                "archive_type": "binary",
            }
        )
        self.safe_patch("shutil.which", return_value=None)
        self.safe_patch("platform.system", return_value="Linux")
        self.safe_patch("platform.machine", return_value="x86_64")
        mock_urlopen = self.safe_patch("urllib.request.urlopen")

        with self.assertRaisesRegex(UserError, "non-public address"):
            manifest.ensure_executable("internalbin")
        mock_urlopen.assert_not_called()

    def test_download_rejects_a_redirect_downgrading_to_http(self):
        # Tests [@ANCHOR: binary_utils_download_and_extract]
        manifest = self.env["binary.manifest"].create(
            {
                "name": "redirectbin_http",
                "url": "https://8.8.8.8/redirectbin_http",
                "checksum": hashlib.sha256(b"data").hexdigest(),
                "archive_type": "binary",
            }
        )
        self.safe_patch("shutil.which", return_value=None)
        self.safe_patch("platform.system", return_value="Linux")
        self.safe_patch("platform.machine", return_value="x86_64")

        mock_response = MagicMock()
        del mock_response.readinto
        mock_response.read.side_effect = [b"data", b""]
        mock_response.getheader.return_value = None
        mock_response.geturl.return_value = "http://8.8.8.8/redirectbin_http"
        mock_response.__enter__.return_value = mock_response
        mock_urlopen = self.safe_patch("urllib.request.urlopen")
        mock_urlopen.return_value = mock_response

        with self.assertRaisesRegex(UserError, "non-https"):
            manifest.ensure_executable("redirectbin_http")

    def test_download_rejects_a_redirect_landing_on_an_internal_address(self):
        # Tests [@ANCHOR: binary_utils_download_and_extract]
        manifest = self.env["binary.manifest"].create(
            {
                "name": "redirectbin_ip",
                "url": "https://8.8.8.8/redirectbin_ip",
                "checksum": hashlib.sha256(b"data").hexdigest(),
                "archive_type": "binary",
            }
        )
        self.safe_patch("shutil.which", return_value=None)
        self.safe_patch("platform.system", return_value="Linux")
        self.safe_patch("platform.machine", return_value="x86_64")

        mock_response = MagicMock()
        del mock_response.readinto
        mock_response.read.side_effect = [b"data", b""]
        mock_response.getheader.return_value = None
        mock_response.geturl.return_value = "https://169.254.169.254/evil"
        mock_response.__enter__.return_value = mock_response
        mock_urlopen = self.safe_patch("urllib.request.urlopen")
        mock_urlopen.return_value = mock_response

        with self.assertRaisesRegex(UserError, "non-public address"):
            manifest.ensure_executable("redirectbin_ip")
