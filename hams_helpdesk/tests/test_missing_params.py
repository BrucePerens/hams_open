# Copyright © Bruce Perens K6BP.
# SPDX-License-Identifier: AGPL-3.0-or-later
"""ir.http._dispatch: a page route called without a parameter its handler requires is a 400 (QA-014)."""
from odoo.tests.common import tagged
from odoo.addons.zero_sudo.tests.common import HamsHttpCase

# The six public GETs of docs/qa/QA_PASS_2026-10-13.md QA-014 that answered 500 with no parameters. A route whose
# module is not installed in the test database is skipped; /scoped_app (web) always exists.
QA_014_ROUTES = (
    "/forum/get_tags",
    "/payment/confirmation",
    "/profile/validate_email",
    "/scoped_app",
    "/scoped_app_icon_png",
    "/shop/pricelist",
)


@tagged("post_install", "-at_install")
class TestMissingRequiredParams(HamsHttpCase):
    def _installed(self, path):
        rules = self.env["ir.http"].routing_map().bind("").match
        try:
            rules(path, method="GET")
        except Exception:  # audit-ignore-catch-all: NotFound or MethodNotAllowed both mean "not served here"
            return False
        return True

    def test_a_bare_get_of_each_qa_014_route_is_a_400_not_a_500(self):
        # Tests [@ANCHOR: hams_helpdesk:missing_required_params_is_400]
        checked = 0
        for path in QA_014_ROUTES:
            if not self._installed(path):
                continue
            checked += 1
            response = self.url_open(path, allow_redirects=False)
            self.assertEqual(response.status_code, 400, path)
        self.assertGreaterEqual(checked, 1, "/scoped_app (web) must always be served")

    def test_the_400_names_the_missing_parameter_and_leaks_no_traceback(self):
        # Tests [@ANCHOR: hams_helpdesk:missing_required_params_is_400]
        response = self.url_open("/scoped_app", allow_redirects=False)
        self.assertEqual(response.status_code, 400)
        self.assertIn("app_id", response.text)
        self.assertNotIn("Traceback", response.text)
        self.assertNotIn("TypeError", response.text)

    def test_a_request_that_carries_the_parameter_reaches_the_handler(self):
        # Tests [@ANCHOR: hams_helpdesk:missing_required_params_is_400]
        response = self.url_open("/scoped_app?app_id=web", allow_redirects=False)
        self.assertEqual(response.status_code, 200)

    def test_a_bare_request_makes_no_server_exception_ticket(self):
        # Tests [@ANCHOR: hams_helpdesk:missing_required_params_is_400]
        # A 400 is an HTTPException, which the reporter already skips (exception_scrub); this keeps it so.
        before = self.env["hams_helpdesk.exception.fingerprint"].search_count([])
        self.url_open("/scoped_app", allow_redirects=False)
        self.env.cr.rollback()
        self.assertEqual(self.env["hams_helpdesk.exception.fingerprint"].search_count([]), before)
