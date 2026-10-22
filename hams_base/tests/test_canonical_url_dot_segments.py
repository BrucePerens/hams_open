# -*- coding: utf-8 -*-
# This file is part of hams_open, an open source module.
# License: AGPL-3.0
import requests

from odoo.tests import tagged

from odoo.addons.hams_base.models.ir_http import _has_dot_segment
from odoo.addons.zero_sudo.tests.common import HamsHttpCase

# Paths a scanner sends. Each reaches Odoo's website 404 page with its dot segments (or bytes that are
# not UTF-8) still in the request path.
HOSTILE_PATHS = [
    "/foo/../x",
    "/foo/%2e%2e/x",
    "/foo/%2E%2E/x",
    "/foo/./x",
    "/a/b/../../../../etc/passwd",
    "/x/..%2fy",
    "/a/%ff/b",
]

# Double-encoded dot segments: the request path is decoded once, so Odoo sees "%2e%2e" in the path and the
# strict URL joiner (which decodes again) refuses it. Exception ticket 74 was /@fs/<x>/self/environ.
DOUBLE_ENCODED_PATHS = [
    "/@fs/%252e%252e/%252e%252e/self/environ",
    "/@fs/%252e%252e%252f%252e%252e%252fself/environ",
    "/@fs/a/%252E%252E/b",
    "/foo/%252e/x",
]


@tagged("post_install", "-at_install")
class TestCanonicalUrlSurvivesDotSegments(HamsHttpCase):
    """Tests [@ANCHOR: hams_base:canonical_url_survives_dot_segments].

    The request is prepared by hand and its URL replaced afterwards, because `requests` (like a browser)
    removes dot segments from a URL it is given; a scanner does not."""

    def _get_raw(self, raw_path):
        # What `Opener.request` does before it sends (flush and clear the test transaction, enable requests).
        self.cr.flush()
        self.cr.clear()
        with self.allow_requests():
            prepared = self.opener.prepare_request(requests.Request("GET", self.base_url() + "/"))
            prepared.url = self.base_url() + raw_path
            return self.opener.send(prepared, timeout=10, allow_redirects=False)

    def test_01_error_page_for_a_dot_segment_path_is_a_clean_404(self):
        # [@ANCHOR: hams_base:test_canonical_url_survives_dot_segments]
        # ---
        # # Verified by [@ANCHOR: hams_base:test_canonical_url_survives_dot_segments]
        for raw_path in HOSTILE_PATHS:
            with self.subTest(path=raw_path):
                with self.assertNoLogs("odoo.http", "ERROR"), self.assertNoLogs(
                    "odoo.addons.http_routing.models.ir_http", "ERROR"
                ):
                    response = self._get_raw(raw_path)
                self.assertEqual(
                    response.status_code,
                    404,
                    f"[!] DIAGNOSTIC FOR AI: {raw_path!r} must answer a clean 404, not a server error. "
                    "See hams_base/models/ir_http.py _url_localized().",
                )

    def test_02_the_canonical_link_is_the_site_root_for_such_a_path(self):
        response = self._get_raw("/foo/%2e%2e/x")
        self.assertEqual(response.status_code, 404)
        self.assertIn('rel="canonical" href="https://hams.com/"', response.text)

    def test_03_a_normal_unknown_page_keeps_its_own_canonical_link(self):
        response = self._get_raw("/no-such-page-here")
        self.assertEqual(response.status_code, 404)
        self.assertIn('rel="canonical" href="https://hams.com/no-such-page-here"', response.text)

    def test_04_double_encoded_dot_segments_are_a_clean_404(self):
        # [@ANCHOR: hams_base:test_url_for_survives_dot_segments]
        # ---
        # # Verified by [@ANCHOR: hams_base:url_for_survives_dot_segments]
        for raw_path in DOUBLE_ENCODED_PATHS:
            with self.subTest(path=raw_path):
                with self.assertNoLogs("odoo.http", "ERROR"), self.assertNoLogs(
                    "odoo.addons.http_routing.models.ir_http", "ERROR"
                ):
                    response = self._get_raw(raw_path)
                self.assertEqual(
                    response.status_code,
                    404,
                    f"[!] DIAGNOSTIC FOR AI: {raw_path!r} must answer a clean 404, not a server error. "
                    "See hams_base/models/ir_http.py _url_for().",
                )

    def test_05_the_dot_segment_check_matches_odoos_own_refusals(self):
        for refused in ("/a/../b", "/a/./b", "/a/%2e%2e/b", "/a/%2E/b", "/a/..%2fb", "/a/%ff/b"):
            self.assertTrue(_has_dot_segment(refused), refused)
        for fine in ("/", "/a/b", "/a/b..c/", "/a/.hidden", "/a/%252e%252e/b", "/caf%C3%A9"):
            self.assertFalse(_has_dot_segment(fine), fine)
