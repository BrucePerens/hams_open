# -*- coding: utf-8 -*-
# SPDX-License-Identifier: AGPL-3.0-or-later
# Part of Odoo. See LICENSE file for full copyright and licensing details.
#
# This file is part of the HAMS project and is licensed under the AGPL-3.0-or-later license.
# See the LICENSE file in the project root for full license information.
import glob
import os
import logging
import io

from odoo.addons.zero_sudo.tests.common import HamsHttpCase
from odoo.tests import tagged
from odoo import tools

_logger = logging.getLogger(__name__)


@tagged("post_install", "-at_install")
class TestBinaryDownloaderTour(HamsHttpCase):
    # [@ANCHOR: test_binary_install_tour]
    def setUp(self):
        super().setUp()
        # Force the admin user to use a deterministic US English locale
        # to prevent headless browser translation crashes during UI tours.
        self.env.ref("base.user_admin").lang = "en_US"

    def tearDown(self):
        super().tearDown()
        # A real, previously-undiscovered bug found live: the file this
        # tour's own successful install actually writes is named via
        # _get_target_filename() -- "tourbin_<checksum-derived-hash>",
        # never literally "tourbin" -- so this cleanup never matched
        # anything and a real download left a permanent leftover file on
        # disk (confirmed directly: found a real stale tourbin_<hash>
        # file from an earlier run). That leftover then poisoned every
        # later run of this same tour: _compute_is_installed() found it
        # already present and the "Install Now" button never appeared
        # the same way again, which looked exactly like nondeterministic
        # browser-timing flakiness until traced back to this. Glob for
        # the real prefix instead of a literal, never-matching filename.
        data_dir = tools.config.get("data_dir", "/var/lib/odoo")
        for path in glob.glob(os.path.join(data_dir, "hams_bin", "tourbin*")):
            try:
                os.remove(path)
            except OSError as e:
                _logger.warning("Could not remove test binary %s: %s", path, e)

    def test_binary_install_tour(self):
        # [!] DIAGNOSTIC FOR AI: Running UI tour for binary installation.
        # Tests [@ANCHOR: UX_BINARY_INSTALL]
        class MockResponse(io.BytesIO):
            def getheader(self, name, default=None):
                return None

        # _download_and_extract() makes two real urlopen() calls (a HEAD
        # pre-check, then the actual GET) -- a shared return_value here
        # would hand back the SAME object both times, and the first
        # `with urlopen(...):` block already closed it by the time the
        # second call tries to read it (io.BytesIO raises "I/O operation
        # on closed file" once closed). side_effect gives each call its
        # own fresh instance instead.
        #
        # b"1234", not b"dummy": the tour's own JS
        # (static/tests/tours/binary_install_tour.js) types
        # "03ac674216f3e15c761ee1a5e255f067953623c8b388b4459e13f978d7c846f4"
        # into the checksum field -- the real SHA-256 of the literal
        # string "1234" -- so the mocked download must serve that same
        # content or action_install()'s own checksum verification
        # rejects it as a mismatch, exactly what happened before this
        # fix (confirmed: b"dummy" hashes to b5a2c962..., not 03ac6742...).
        self.safe_patch(
            "urllib.request.urlopen",
            side_effect=lambda *args, **kwargs: MockResponse(b"1234"),
        )
        url_action = "/odoo?debug=1&action=binary_downloader.action_binary_downloader_manifest"
        self.start_tour(url_action, "binary_install_tour", login="admin")

        # action_install()'s own display_notification+reload chain isn't
        # observable from this headless tour harness (see the comment in
        # binary_install_tour.js), so the JS tour itself only proves the
        # click's RPC succeeded with no error. The real outcome --
        # ensure_executable() actually installing the binary to the
        # central pool -- is asserted here directly.
        manifest = self.env["binary.manifest"].search([("name", "=", "tourbin")], limit=1)
        self.assertTrue(manifest, "The tour did not create a binary.manifest record.")
        manifest.invalidate_recordset(["is_installed"])
        self.assertTrue(
            manifest.is_installed,
            "action_install() did not actually install the binary.",
        )
