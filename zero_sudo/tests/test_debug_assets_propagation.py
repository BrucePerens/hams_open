# -*- coding: utf-8 -*-
# SPDX-License-Identifier: AGPL-3.0-or-later
"""`?debug=assets` must reach the web client's own page bundles.

JS coverage (`HAMS_JS_COVERAGE_DIR` + `HAMS_TOUR_TOUR_DEBUG=assets`, see
`_js_coverage_start` in common.py and hams_shared/tools/run_js_coverage.py)
maps V8 block ranges back to source lines through the `Filepath:` headers
that only the non-minified `debug=assets` bundles carry. The live records
from 2026-09-20 (`-u pager_duty`, tour URL rewritten to
`/odoo?debug=assets`) show both `web.assets_web` and `web.assets_tests`
served as `/web/assets/<hash>/<bundle>.min.js` from the same page render,
so `values["debug"]` lacked "assets" when `web.webclient_bootstrap` was
rendered. No hams module overrides `_handle_debug`, `session.debug`, the
`/odoo` route or the asset-link generation (checked 2026-10-02 in all
three repositories), so this test pins the server half of the path:
the same authenticated `/odoo?debug=assets` request a tour starts with,
over the real HTTP stack, and the follow-up request without the query
parameter that a page navigation inside the tour makes. If both pass,
the cause is on the browser/tour side, not in request handling.
hams_com night_shift_todo/low/js-coverage-live-v8-collection-hook-aa0f3a74.md
"""
import re

from odoo.tests.common import tagged

from odoo.addons.zero_sudo.tests.common import HamsHttpCase

# website's ir.asset inserts the website id: /web/assets/<id>/debug/...
_DEBUG_WEB_BUNDLE_RE = re.compile(
    r'/web/assets/(?:\d+/)?debug/web\.assets_web\.js"'
)
_MIN_WEB_BUNDLE_RE = re.compile(
    r'/web/assets/(?:\d+/)?[0-9a-f]+/web\.assets_web\.min\.js"'
)


@tagged("post_install", "-at_install")
class TestDebugAssetsReachesWebClientBundles(HamsHttpCase):
    def _webclient_html(self, url_path):
        response = self.url_open(url_path)
        self.assertEqual(
            response.status_code,
            200,
            f"{url_path} answered {response.status_code} "
            f"(final URL {response.url})",
        )
        return response.text

    def test_debug_assets_serves_unminified_web_bundle(self):
        self.authenticate("admin", "admin")
        html = self._webclient_html("/odoo?debug=assets")
        self.assertRegex(
            html,
            _DEBUG_WEB_BUNDLE_RE,
            "[!] DIAGNOSTIC FOR AI: /odoo?debug=assets rendered "
            "web.webclient_bootstrap without the debug web.assets_web "
            "bundle, so request.session.debug did not contain 'assets' "
            "at render time. JS coverage cannot map minified bundles "
            "to source lines.",
        )
        self.assertNotRegex(html, _MIN_WEB_BUNDLE_RE)

    def test_debug_assets_persists_in_session_for_next_page(self):
        self.authenticate("admin", "admin")
        self._webclient_html("/odoo?debug=assets")
        html = self._webclient_html("/odoo")
        self.assertRegex(
            html,
            _DEBUG_WEB_BUNDLE_RE,
            "[!] DIAGNOSTIC FOR AI: debug=assets was set on the first "
            "request but a second /odoo request in the same session "
            "got the minified bundle: the session did not keep "
            "'debug'. A tour that navigates to a new document loses "
            "the debug bundles this way.",
        )

    def test_plain_debug_mode_keeps_minified_bundle(self):
        # The control: debug=1 (what the tours pass unless
        # HAMS_TOUR_TOUR_DEBUG overrides it) is not asset debug, so the
        # two regexes above really distinguish the two bundle shapes.
        self.authenticate("admin", "admin")
        html = self._webclient_html("/odoo?debug=1")
        self.assertRegex(html, _MIN_WEB_BUNDLE_RE)
        self.assertNotRegex(html, _DEBUG_WEB_BUNDLE_RE)
