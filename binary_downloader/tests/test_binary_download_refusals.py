# -*- coding: utf-8 -*-
# SPDX-License-Identifier: AGPL-3.0-or-later
# Part of Odoo. See LICENSE file for full copyright and licensing details.
#
# This file is part of the HAMS project and is licensed under the AGPL-3.0-or-later license.
# See the LICENSE file in the project root for full license information.
import hashlib
import io
import os
import tarfile
import zipfile

from odoo import tools
from odoo.exceptions import UserError
from odoo.tests.common import tagged
from odoo.addons.zero_sudo.tests.common import HamsTransactionCase

_UTILS = "odoo.addons.binary_downloader.models.binary_utils"


class _Response(io.BytesIO):
    def getheader(self, name, default=None):
        return None


@tagged("post_install", "-at_install")
class TestBinaryDownloadRefusals(HamsTransactionCase):
    """Refusals of _download_and_extract / _unlink_binary_file: scheme, path escape, checksum, archive member."""

    def setUp(self):
        super().setUp()
        self.manifest = self.env["binary.manifest"]
        self.bin_dir = os.path.join(tools.config.get("data_dir", "/var/lib/odoo"), "hams_bin")
        self.safe_patch("platform.system", return_value="Linux")
        self.safe_patch("platform.machine", return_value="x86_64")
        self.safe_patch(f"{_UTILS}.BinaryDownloaderMixin._assert_host_is_ssrf_safe", return_value=None)

    def _serve(self, payload):
        return self.safe_patch(f"{_UTILS}._urlopen_ssrf_safe", side_effect=lambda *a, **k: _Response(payload))

    def _target(self, name, checksum):
        path = os.path.join(self.bin_dir, self.manifest._get_target_filename(name, checksum))
        self.addCleanup(lambda: os.path.lexists(path) and os.unlink(path))
        return path

    def _tar(self, member):
        stream = io.BytesIO()
        with tarfile.open(fileobj=stream, mode="w:gz") as tar:  # audit-ignore-path  # fmt: skip
            info = tarfile.TarInfo(name=member)
            info.size = 3
            tar.addfile(info, io.BytesIO(b"abc"))
        return stream.getvalue()

    def _zip(self, member):
        stream = io.BytesIO()
        with zipfile.ZipFile(stream, "w") as archive:  # audit-ignore-path  # fmt: skip
            archive.writestr(member, b"abc")
        return stream.getvalue()

    def test_plain_http_url_is_refused_before_any_request(self):
        # Tests [@ANCHOR: binary_utils_download_and_extract]
        urlopen = self.safe_patch(f"{_UTILS}._urlopen_ssrf_safe")
        with self.assertRaises(UserError):
            self.manifest._download_and_extract("refusal_tool", "http://example.com/x", "0" * 64, "binary")
        urlopen.assert_not_called()

    def test_download_name_with_parent_segment_is_refused(self):
        # Tests [@ANCHOR: binary_utils_download_and_extract]
        urlopen = self.safe_patch(f"{_UTILS}._urlopen_ssrf_safe")
        with self.assertRaises(UserError) as caught:
            self.manifest._download_and_extract("a/../b", "https://example.com/x", "0" * 64, "binary")
        self.assertIn("Path traversal", str(caught.exception))
        urlopen.assert_not_called()

    def test_download_absolute_name_cannot_leave_the_binary_directory(self):
        # Tests [@ANCHOR: binary_utils_download_and_extract]
        urlopen = self.safe_patch(f"{_UTILS}._urlopen_ssrf_safe")
        with self.assertRaises(UserError) as caught:
            self.manifest._download_and_extract(os.path.join(os.sep, "etc", "refusal_tool"), "https://example.com/x", "0" * 64, "binary")
        self.assertIn("Path traversal", str(caught.exception))
        urlopen.assert_not_called()

    def test_checksum_mismatch_installs_nothing(self):
        # Tests [@ANCHOR: binary_utils_download_and_extract]
        wrong = "1" * 64
        target = self._target("refusal_tool", wrong)
        self._serve(b"not the promised bytes")
        with self.assertRaises(UserError) as caught:
            self.manifest._download_and_extract("refusal_tool", "https://example.com/x", wrong, "binary")
        self.assertIn("Checksum mismatch", str(caught.exception))
        self.assertFalse(os.path.lexists(target))

    def test_tar_without_the_wanted_member_is_refused(self):
        # Tests [@ANCHOR: binary_utils_download_and_extract]
        payload = self._tar("other_tool")
        checksum = hashlib.sha256(payload).hexdigest()
        target = self._target("refusal_tool", checksum)
        self._serve(payload)
        with self.assertRaises(UserError) as caught:
            self.manifest._download_and_extract("refusal_tool", "https://example.com/x", checksum, "tar.gz")
        self.assertIn("not found in archive", str(caught.exception))
        self.assertFalse(os.path.lexists(target))

    def test_zip_without_the_wanted_member_is_refused(self):
        # Tests [@ANCHOR: binary_utils_download_and_extract]
        payload = self._zip("other_tool")
        checksum = hashlib.sha256(payload).hexdigest()
        target = self._target("refusal_tool", checksum)
        self._serve(payload)
        with self.assertRaises(UserError) as caught:
            self.manifest._download_and_extract("refusal_tool", "https://example.com/x", checksum, "zip")
        self.assertIn("not found in zip archive", str(caught.exception))
        self.assertFalse(os.path.lexists(target))

    def test_installed_binary_with_wrong_content_is_replaced_not_trusted(self):
        # Tests [@ANCHOR: binary_utils_download_and_extract]
        payload = b"the real bytes"
        checksum = hashlib.sha256(payload).hexdigest()
        target = self._target("refusal_tool", checksum)
        os.makedirs(self.bin_dir, exist_ok=True)
        with open(target, "wb") as tampered:  # audit-ignore-path  # fmt: skip
            tampered.write(b"tampered")
        self._serve(payload)
        path = self.manifest._download_and_extract("refusal_tool", "https://example.com/x", checksum, "binary")
        self.assertEqual(path, target)
        with open(target, "rb") as installed:  # audit-ignore-path  # fmt: skip
            self.assertEqual(installed.read(), payload)

    def test_unlink_refuses_parent_segment_and_absolute_names(self):
        # Tests [@ANCHOR: binary_utils_unlink_binary_file]
        victim = os.path.join(self.bin_dir, "refusal_victim")
        os.makedirs(self.bin_dir, exist_ok=True)
        with open(victim, "wb") as handle:  # audit-ignore-path  # fmt: skip
            handle.write(b"keep")
        self.addCleanup(lambda: os.path.lexists(victim) and os.unlink(victim))
        with self.assertRaises(UserError):
            self.manifest._unlink_binary_file("../refusal_victim", "0" * 64)
        with self.assertRaises(UserError):
            self.manifest._unlink_binary_file(os.path.join(os.sep, "etc", "refusal_tool"), "0" * 64)
        self.assertTrue(os.path.exists(victim))
