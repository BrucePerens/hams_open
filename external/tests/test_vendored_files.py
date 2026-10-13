# Copyright © Bruce Perens K6BP.
# SPDX-License-Identifier: AGPL-3.0-or-later

# -*- coding: utf-8 -*-
import hashlib
import os
import re

from odoo.addons.zero_sudo.tests.common import HamsTransactionCase
from odoo.tests import tagged

MODULE_ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
NODE_MODULES = os.path.join(MODULE_ROOT, "static", "src", "node_modules")


@tagged("post_install", "-at_install", "external")
class TestVendoredFiles(HamsTransactionCase):
    """jsPDF and fabric.js (QA-015) are vendored unmodified: VENDORED.md records the sha256 of each file."""

    ROW = re.compile(r"`((?:jspdf|fabric)/[A-Za-z0-9_.-]+)`[^\n]*?`([0-9a-f]{64})`")

    def _recorded(self):
        with open(os.path.join(MODULE_ROOT, "VENDORED.md"), encoding="utf-8") as handle:
            return dict(self.ROW.findall(handle.read()))

    def test_every_recorded_file_matches_its_sha256(self):
        recorded = self._recorded()
        self.assertEqual(len(recorded), 5, recorded)
        for path, digest in recorded.items():
            with open(os.path.join(NODE_MODULES, path), "rb") as handle:
                self.assertEqual(hashlib.sha256(handle.read()).hexdigest(), digest, path)

    def test_every_file_of_the_two_libraries_is_recorded(self):
        on_disk = {
            "%s/%s" % (library, name)
            for library in ("jspdf", "fabric")
            for name in os.listdir(os.path.join(NODE_MODULES, library))
        }
        self.assertEqual(on_disk, set(self._recorded()))

    def test_each_library_ships_its_licence(self):
        for library in ("jspdf", "fabric"):
            with open(os.path.join(NODE_MODULES, library, "LICENSE"), encoding="utf-8") as handle:
                self.assertIn("Permission is hereby granted, free of charge", handle.read(), library)
