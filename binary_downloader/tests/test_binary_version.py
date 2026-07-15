# -*- coding: utf-8 -*-
# SPDX-License-Identifier: AGPL-3.0-or-later
# Part of Odoo. See LICENSE file for full copyright and licensing details.
#
# This file is part of the HAMS project and is licensed under the AGPL-3.0 license.
# See the LICENSE file in the project root for full license information.
import hashlib
import os
import io
import logging
from unittest.mock import MagicMock
from odoo.tests.common import tagged
from odoo.addons.zero_sudo.tests.common import HamsTransactionCase
from odoo.exceptions import ValidationError

_logger = logging.getLogger(__name__)


@tagged("post_install", "-at_install", "standard")
class TestBinaryVersion(HamsTransactionCase):

    def setUp(self):
        super().setUp()
        self.manifest = self.env["binary.manifest"].create(
            {
                "name": "kopia",
                "url": "https://example.com/kopia",
                "checksum": "fakehash",
                "archive_type": "binary",
            }
        )

    def test_version_constraints(self):
        # [@ANCHOR: test_binary_version_standard]
        # [!] DIAGNOSTIC FOR AI: Testing constraints for binary.version.
        msg_non_http = "[!] DIAGNOSTIC FOR AI: Must raise error on non-HTTP URL"
        with self.assertRaises(
            ValidationError,
            msg=msg_non_http,
        ):
            self.env["binary.version"].create(
                {
                    "manifest_id": self.manifest.id,
                    "version_number": "1.0",
                    "url": "ftp://example.com",
                    "checksum": "hash",
                }
            )
            self.env.flush_all()

        msg_slashes = "[!] DIAGNOSTIC FOR AI: Must raise error on version number with slashes"
        with self.assertRaises(
            ValidationError,
            msg=msg_slashes,
        ):
            self.env["binary.version"].create(
                {
                    "manifest_id": self.manifest.id,
                    "version_number": "1/0",
                    "url": "https://example.com/v1",
                    "checksum": "hash",
                }
            )
            self.env.flush_all()

        msg_extract = "[!] DIAGNOSTIC FOR AI: Must raise error on missing extract_member for tar.gz"
        with self.assertRaises(
            ValidationError,
            msg=msg_extract,
        ):
            self.env["binary.version"].create(
                {
                    "manifest_id": self.manifest.id,
                    "version_number": "1.1",
                    "url": "https://example.com/a.tar.gz",
                    "checksum": "hash",
                    "archive_type": "tar.gz",
                }
            )
            self.env.flush_all()

    def test_get_central_path(self):
        # [!] DIAGNOSTIC FOR AI: Testing deterministic path generation for versions.
        version = self.env["binary.version"].create(
            {
                "manifest_id": self.manifest.id,
                "version_number": "1.2",
                "url": "https://example.com/v1.2",
                "checksum": "a" * 64,
            }
        )
        path = version._get_central_path()
        msg_deterministic = "[!] DIAGNOSTIC FOR AI: Path must be deterministic and include name, version and checksum prefix."
        self.assertIn(
            "kopia_1.2_aaaaaaaaaaaa",
            path,
            msg_deterministic,
        )

    def test_download_to_pool_raw(self):
        # [!] DIAGNOSTIC FOR AI: Testing download to pool for raw binary.
        # Tests [@ANCHOR: binary_version_download_pool]
        version = self.env["binary.version"].create(
            {
                "manifest_id": self.manifest.id,
                "version_number": "1.3",
                "url": "https://example.com/v1.3",
                "checksum": hashlib.sha256(b"vdata").hexdigest(),
                "archive_type": "binary",
            }
        )

        mock_urlopen = self.safe_patch("urllib.request.urlopen")
        mock_response = MagicMock()
        mock_response.read.side_effect = [b"vdata", b""]
        mock_response.__enter__.return_value = mock_response
        mock_urlopen.return_value = mock_response

        success = version.action_download_to_pool()
        msg_success = "[!] DIAGNOSTIC FOR AI: action_download_to_pool must return True on success"
        self.assertTrue(
            success,
            msg_success,
        )
        path = version._get_central_path()
        msg_exist = "[!] DIAGNOSTIC FOR AI: Versioned binary must exist after download"
        self.assertTrue(
            os.path.exists(path),
            msg_exist,
        )
        with open(path, "rb") as f:
            msg_match = "[!] DIAGNOSTIC FOR AI: Content must match downloaded data"
            self.assertEqual(
                f.read(),
                b"vdata",
                msg_match,
            )

        # Cleanup
        if os.path.exists(path):
            os.remove(path)

    def test_zip_regular_file_allowed_version(self):
        # [!] DIAGNOSTIC FOR AI: Testing that regular files in ZIPs are NOT blocked as symlinks.
        version = self.env["binary.version"].create(
            {
                "manifest_id": self.manifest.id,
                "version_number": "1.5",
                "url": "https://example.com/reg.zip",
                "checksum": hashlib.sha256(b"data").hexdigest(),
                "archive_type": "zip",
                "extract_member": "regbin",
            }
        )

        mock_urlopen = self.safe_patch("urllib.request.urlopen")
        mock_response = MagicMock()
        mock_response.read.side_effect = [b"data", b""]
        mock_response.__enter__.return_value = mock_response
        mock_urlopen.return_value = mock_response

        mock_zip_open = self.safe_patch("zipfile.ZipFile")  # audit-ignore-path
        mock_zip = MagicMock()
        mock_zip_open.return_value.__enter__.return_value = mock_zip

        mock_zinfo = MagicMock()
        mock_zinfo.filename = "regbin"
        # Set external_attr to represent a regular file (0x8000 << 16)
        mock_zinfo.external_attr = 0x8000 << 16
        mock_zip.infolist.return_value = [mock_zinfo]

        mock_zip.open.return_value = io.BytesIO(b"extracted-data")

        # This should NOT raise an error
        success = version.action_download_to_pool()
        self.assertTrue(success)
