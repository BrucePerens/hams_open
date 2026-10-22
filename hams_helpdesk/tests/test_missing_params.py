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
        self.assertEqual(self.env["hams_helpdesk.exception.fingerprint"].search_count([]), before)

    # -- QA pass 2 (2026-10-14): the same 400 for a JSON-RPC route, and for a NUL byte --

    def _rpc(self, route, params):
        return self.opener.post(
            self.base_url() + route,
            json={"id": 0, "jsonrpc": "2.0", "method": "call", "params": params},
        )

    def test_a_json_call_that_leaves_out_a_required_argument_is_a_400_and_no_ticket(self):
        # Tests [@ANCHOR: hams_helpdesk:missing_required_params_is_400]
        # /web/session/authenticate declares db, login and password as required positionals; a call with none
        # raised TypeError inside the handler, an exception ticket per call.
        before = self.env["hams_helpdesk.exception.fingerprint"].search_count([])
        body = self._rpc("/web/session/authenticate", {}).json()
        self.assertIn("error", body)
        # Odoo's JSON-RPC dispatcher puts 0 in error.code for every exception but NotFound and a session
        # expiry (the nightly of 2026-10-18 says "we don't care of this code"); the 400 is the exception's own
        # class and message in error.data.
        self.assertEqual(body["error"]["data"]["name"], "werkzeug.exceptions.BadRequest", body)
        self.assertIn("400 Bad Request", body["error"]["data"]["message"])
        self.assertIn("Missing required parameter", str(body))
        self.assertNotIn("Traceback", str(body))
        self.assertNotIn("TypeError", str(body))
        self.assertEqual(self.env["hams_helpdesk.exception.fingerprint"].search_count([]), before)

    def test_a_json_call_that_carries_its_arguments_still_works(self):
        # Tests [@ANCHOR: hams_helpdesk:missing_required_params_is_400]
        self.authenticate("admin", "admin")
        params = {"model": "res.partner", "method": "search_count", "args": [[]], "kwargs": {}}
        body = self._rpc("/web/dataset/call_kw/res.partner.search_count", params).json()
        self.assertIn("result", body, body)

    def test_a_nul_byte_in_a_query_parameter_is_a_400_not_a_500(self):
        # Tests [@ANCHOR: hams_helpdesk:request_has_nul_byte]
        for route in ("/web/login?redirect=a%00b", "/scoped_app?app_id=a%00b"):
            response = self.url_open(route, allow_redirects=False)
            self.assertEqual(response.status_code, 400, route)
            self.assertNotIn("Traceback", response.text)

    def test_a_nul_byte_in_the_path_is_a_400_not_a_500(self):
        # Tests [@ANCHOR: hams_helpdesk:nul_byte_in_path]
        for route in ("/no%00such-page", "/member/a%00b", "/w1aw%00/blog"):
            response = self.url_open(route, allow_redirects=False)
            self.assertEqual(response.status_code, 400, route)

    def test_a_nul_byte_in_a_json_body_is_refused(self):
        # Tests [@ANCHOR: hams_helpdesk:request_has_nul_byte]
        self.authenticate("admin", "admin")
        params = {"model": "res.partner", "method": "search_count", "args": [[["name", "=", "a\u0000b"]]], "kwargs": {}}
        body = self._rpc("/web/dataset/call_kw/res.partner.search_count", params).json()
        self.assertEqual(body["error"]["data"]["name"], "werkzeug.exceptions.BadRequest", body)

    def test_text_where_a_handler_wants_a_number_is_a_400_not_a_500(self):
        # Tests [@ANCHOR: hams_helpdesk:missing_required_params_is_400]
        # Stock /web/image runs int(width): `?width=abc` raised ValueError, a 500 and a ticket for anyone.
        before = self.env["hams_helpdesk.exception.fingerprint"].search_count([])
        for route in (
            "/web/image/res.partner/1/avatar_128?width=abc",
            "/web/image?model=res.partner&id=1&field=avatar_128&height=1e9x",
        ):
            response = self.url_open(route, allow_redirects=False)
            self.assertEqual(response.status_code, 400, route)
            self.assertNotIn("Traceback", response.text)
            self.assertNotIn("ValueError", response.text)
        self.assertEqual(self.env["hams_helpdesk.exception.fingerprint"].search_count([]), before)

    def test_text_where_a_blog_wants_a_date_is_a_400_not_a_500(self):
        # Tests [@ANCHOR: hams_helpdesk:bad_date_is_400]
        # Tests [@ANCHOR: hams_helpdesk:error_is_unreadable_date]
        # 2026-10-16: /blog?...&date_begin=ijyM&date_end=pNOE and /blog/<blog>?... were 500s and tickets (72, 73)
        # for a scanner. /blog redirects to the blog itself when only one exists, as the date is read only when
        # there are several (production has two), so the test makes two.
        self.assertTrue(self._installed("/blog"), "stock /blog (website_blog) must be installed")
        blogs = self.env["blog.blog"].create([{"name": "Date test one"}, {"name": "Date test two"}])
        self.assertGreater(self.env["blog.blog"].search_count([]), 1)
        slug = self.env["ir.http"]._slug(blogs[0])
        before = self.env["hams_helpdesk.exception.fingerprint"].search_count([])
        for route in (
            "/blog?date_begin=ijyM&date_end=pNOE",
            "/blog?date_begin=2026-13-45",
            f"/blog/{slug}?date_begin=ijyM&date_end=pNOE",
        ):
            response = self.url_open(route, allow_redirects=False)
            self.assertEqual(response.status_code, 400, route)
            self.assertNotIn("Traceback", response.text)
            self.assertNotIn("ValueError", response.text)
        self.assertEqual(self.env["hams_helpdesk.exception.fingerprint"].search_count([]), before)
        # A real date is still served.
        self.assertEqual(self.url_open("/blog?date_begin=2026-01-01", allow_redirects=False).status_code, 200)

    def test_a_nul_byte_in_the_path_or_a_parameter_is_a_400_not_a_500(self):
        # [@ANCHOR: hams_helpdesk:test_nul_byte_is_400]
        # Tests [@ANCHOR: hams_helpdesk:request_has_nul_byte]
        before = self.env["hams_helpdesk.exception.fingerprint"].search_count([])
        for path in ("/scoped_app?app_id=%00", "/web/login?login=%00"):
            response = self.url_open(path, allow_redirects=False)
            self.assertEqual(response.status_code, 400, path)
            self.assertNotIn("Traceback", response.text)
        self.assertEqual(self.env["hams_helpdesk.exception.fingerprint"].search_count([]), before)
        # Without it the same routes are served.
        self.assertEqual(self.url_open("/scoped_app?app_id=web", allow_redirects=False).status_code, 200)
