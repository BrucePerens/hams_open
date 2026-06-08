# -*- coding: utf-8 -*-
import hashlib
import os
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
                "name": "testvbin",
                "url": "http://example.com/testvbin",
                "checksum": "fakehash",
                "archive_type": "binary",
            }
        )

    def test_version_constraints(self):
        # [@ANCHOR: test_binary_version_standard]
        # [!] DIAGNOSTIC FOR AI: Testing constraints for binary.version.
        with self.assertRaises(
            ValidationError,
            msg="[!] DIAGNOSTIC FOR AI: Must raise error on non-HTTP URL",
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

        with self.assertRaises(
            ValidationError,
            msg="[!] DIAGNOSTIC FOR AI: Must raise error on version number with slashes",
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

        with self.assertRaises(
            ValidationError,
            msg="[!] DIAGNOSTIC FOR AI: Must raise error on missing extract_member for tar.gz",
        ):
            self.env["binary.version"].create(
                {
                    "manifest_id": self.manifest.id,
                    "version_number": "1.1",
                    "url": "http://example.com/a.tar.gz",
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
                "url": "http://example.com/v1.2",
                "checksum": "a" * 64,
            }
        )
        path = version._get_central_path()
        self.assertIn(
            "testvbin_1.2_aaaaaaaaaaaa",
            path,
            "[!] DIAGNOSTIC FOR AI: Path must be deterministic and include name, version and checksum prefix.",
        )

    def test_download_to_pool_raw(self):
        # [!] DIAGNOSTIC FOR AI: Testing download to pool for raw binary.
        # Tests [@ANCHOR: binary_version_download_pool]
        version = self.env["binary.version"].create(
            {
                "manifest_id": self.manifest.id,
                "version_number": "1.3",
                "url": "http://example.com/v1.3",
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
        self.assertTrue(
            success,
            "[!] DIAGNOSTIC FOR AI: action_download_to_pool must return True on success",
        )
        path = version._get_central_path()
        self.assertTrue(
            os.path.exists(path),
            "[!] DIAGNOSTIC FOR AI: Versioned binary must exist after download",
        )
        with open(path, "rb") as f:
            self.assertEqual(
                f.read(),
                b"vdata",
                "[!] DIAGNOSTIC FOR AI: Content must match downloaded data",
            )

        # Cleanup
        if os.path.exists(path):
            os.remove(path)
