# -*- coding: utf-8 -*-
# Copyright © Bruce Perens K6BP.
# SPDX-License-Identifier: AGPL-3.0-or-later
import json
import os

from odoo.modules.module import get_module_path
from odoo.tests import tagged
from odoo.addons.zero_sudo.tests.common import HamsTransactionCase

from odoo.addons.hams_rpc_guard import exposure_scan

REPOSITORY = os.path.dirname(get_module_path("hams_rpc_guard"))


@tagged("post_install", "-at_install")
class TestRepositoryExposure(HamsTransactionCase):
    """The checked-in allowlists match the code of this repository, and the daemons keep working."""
    # Tests [@ANCHOR: hams_rpc_guard:scan_main]
    # Tests [@ANCHOR: hams_rpc_guard:scan_daemon_pairs]

    def test_every_public_method_of_every_addon_is_listed_or_declared(self):
        """A new public method of any model of this repository fails here until it is made private (leading
        underscore or `@api.private`), declared with `@public_rpc`, or the addon's `rpc_exposure.json` is regenerated
        in the same pull request: `python3 hams_rpc_guard/exposure_scan.py --write .`"""
        self.assertEqual(exposure_scan.main(["--check", REPOSITORY]), 0,
                         "rpc_exposure.json is out of date: run python3 hams_rpc_guard/exposure_scan.py --write .")

    def test_every_pair_a_daemon_calls_is_in_that_daemons_allowlist(self):
        checked = 0
        for addon in exposure_scan.managed_addons(REPOSITORY):
            path = os.path.join(REPOSITORY, addon, "rpc_exposure.json")
            if not os.path.isfile(path):
                continue
            with open(path, encoding="utf-8") as handle:
                clients = json.load(handle).get("clients") or {}
            for login, entry in clients.items():
                allowed = {(model, method) for model, methods in entry["allow"].items() for method in methods}
                for source in entry.get("sources", ()):
                    with open(os.path.join(REPOSITORY, source), encoding="utf-8") as handle:
                        called = exposure_scan.daemon_pairs(handle.read())
                    self.assertTrue(called, "%s calls nothing the scanner can see: is the source right?" % source)
                    self.assertEqual(sorted(called - allowed), [], "%s calls methods missing from %s's allowlist" % (source, login))
                    checked += 1
        self.assertGreaterEqual(checked, 4, "the pager, backup and exception-reporter daemons are all covered")
