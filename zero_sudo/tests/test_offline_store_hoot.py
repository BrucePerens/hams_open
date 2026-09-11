# -*- coding: utf-8 -*-
# This software is distributed under the terms of the Affero General Public License (AGPL-3).
# SPDX-License-Identifier: AGPL-3.0-or-later
from odoo.addons.zero_sudo.tests.common import HamsHttpCase
from odoo.tests.common import tagged


def _hoot_unit_test_error_checker(message):
    # Matches ham_shack's own test_auto_scan_hoot.py checker: any non-[HOOT]-
    # branded console error aborts immediately; a [HOOT] failure is left for
    # hoot's own pass/fail reporting via the success signal below.
    return "[HOOT]" not in message


@tagged("post_install", "-at_install")
class TestOfflineStoreHoot(HamsHttpCase):
    # Runs offline_store.test.js's real hoot unit suite in a real browser,
    # scoped via hoot's `tag` URL param to just this suite -- same pattern
    # as ham_shack's own test_auto_scan_hoot.py. Without this, the .test.js
    # file being listed in __manifest__.py's web.assets_unit_tests bundle
    # is not enough on its own -- nothing else in this codebase's test
    # suite ever actually loads /web/tests with a matching tag, so the
    # suite would otherwise never run at all (the exact "vacuous pass"
    # trap ham_shack/__manifest__.py's own comments repeatedly warn about).
    def test_offline_store_hoot_suite_passes(self):
        self.browser_js(
            "/web/tests?headless&loglevel=2&preset=desktop&timeout=15000&tag=zero_sudo_offline_store",
            "",
            "",
            login="admin",
            timeout=120,
            success_signal="[HOOT] Test suite succeeded",
            error_checker=_hoot_unit_test_error_checker,
        )
