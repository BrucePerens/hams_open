# -*- coding: utf-8 -*-
# Copyright © HAMS project. AGPL-3.0-or-later.
import os
import shutil
import subprocess
import unittest

from odoo.tests.common import BaseCase, tagged

_HERE = os.path.dirname(os.path.abspath(__file__))
_SW = os.path.join(_HERE, "..", "static", "src", "sw", "sw.js")
_HARNESS = os.path.join(_HERE, "sw_unit_harness.js")


@tagged("post_install", "-at_install")
class TestServiceWorkerUnit(BaseCase):
    """The service worker's own functions, run by Node in a vm with stand-ins for the worker globals (sw_unit_harness.js)."""

    def _run(self, name):
        node = shutil.which("node")
        if not node:
            raise unittest.SkipTest("node is not installed")
        result = subprocess.run([node, _HARNESS, _SW, name], capture_output=True, text=True, timeout=60, check=False)
        self.assertEqual(result.returncode, 0, f"sw_unit_harness {name} failed:\n{result.stdout}\n{result.stderr}")

    def test_open_db_creates_the_store_once_and_rejects_on_error(self):
        # Tests [@ANCHOR: caching:js_sw_open_db]
        self._run("open_db")

    def test_update_lru_metadata_records_the_url_and_time(self):
        # Tests [@ANCHOR: caching:js_sw_update_lru_metadata]
        self._run("update_lru_metadata")

    def test_enforce_lru_quota_evicts_the_ten_oldest_over_quota(self):
        # Tests [@ANCHOR: caching:js_sw_enforce_lru_quota]
        self._run("enforce_lru_quota")

    def test_previous_version_cache_exists_only_for_an_older_cache_of_its_own(self):
        # Tests [@ANCHOR: caching:js_sw_previous_version_cache_exists]
        self._run("previous_version_cache_exists")
