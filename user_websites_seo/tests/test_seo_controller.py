# This software is distributed under the terms of the Affero General Public License (AGPL-3).

# -*- coding: utf-8 -*-
from odoo.addons.zero_sudo.tests.common import HamsTransactionCase
from odoo.http import Response
from odoo.addons.user_websites_seo.controllers.main import UserWebsitesSEOController


from odoo.tests import tagged


@tagged("post_install", "-at_install")
class TestSEOController(HamsTransactionCase):

    def setUp(self):
        super().setUp()
        self.regular_user = self.env["res.users"].create(
            {
                "name": "SEO Test User",
                "login": "seo_test",
                "website_slug": "seo-test-user",
                "group_ids": [(6, 0, [self.env.ref("base.group_portal").id])],
            }
        )
        self.controller = UserWebsitesSEOController()

    def test_controller_no_ssti_elevation(self):
        # Tests [@ANCHOR: COMM_controller_user_blog_index_seo_override]

        # [@ANCHOR: COMM_test_controller_no_ssti_elevation]
        """
        Verify the controller de-elevates main_object/profile_user to the
        REQUESTING visitor's own environment before QWeb rendering, per
        ADR-0078 -- not merely that it returns *some* object bound to
        whatever env it started with.

        bug-hunt (2026-09-11, non-discriminating-test / bug class 2): the
        original version of this test never installed an
        `odoo.http.request` at all. `odoo.http.request` is a werkzeug
        `LocalProxy` over an empty `LocalStack` outside a real HTTP
        dispatch (confirmed by reading `_request_stack`/`LocalProxy.__bool__`
        in `werkzeug/local.py`: an unbound proxy's `__bool__` uses the
        `fallback=lambda self: False` path, i.e. `bool(request)` is
        `False`, not an exception), so `HamsTransactionCase` -- a plain
        `TransactionCase`, not an `HttpCase` -- never has one bound. The
        controller's own `if request and request.env:` guard therefore
        short-circuited on the FIRST operand and the de-elevation line
        (`user = user.with_user(request.env.user)`) never ran. The old
        assertion (`main_obj.env.uid == self.regular_user.env.uid`) was
        then comparing `self.regular_user`'s own env against itself,
        unmodified -- true whether de-elevation fires, is broken, or is
        deleted outright. Fixed by actually installing a mock `request`
        (matching this repo's own `cloudflare/tests/test_cloudflare_headers.py`
        pattern for mocking `odoo.http.request` in a non-HttpCase test) whose
        `.env.user` is a THIRD user, distinct from both `self.regular_user`
        and this test's own (superuser) env, and asserting the returned
        object's env is actually bound to that third user -- an assertion
        that can only pass if the de-elevation line genuinely executed.
        """
        # A third, distinct, lower-privileged user standing in for "the
        # anonymous/portal visitor actually loading this page" -- deliberately
        # NOT self.regular_user (the profile owner) and NOT this test's own
        # (superuser) env, so the assertion below can't pass by accident.
        visitor = self.env["res.users"].create(
            {
                "name": "SEO Test Visitor",
                "login": "seo_test_visitor",
                "group_ids": [(6, 0, [self.env.ref("base.group_portal").id])],
            }
        )

        mock_super_index = self.safe_patch(
            "odoo.addons.user_websites_seo.controllers.main."
            "UserWebsitesController.user_blog_index"
        )

        # Mock the response from the base controller
        mock_response = Response()
        mock_response.type = "http"
        mock_response.qcontext = {"profile_user": self.regular_user}
        mock_super_index.return_value = mock_response

        # Install a real (mocked) request bound to the visitor's own env, so
        # the controller's `if request and request.env:` guard is actually
        # True and the de-elevation line actually runs.
        mock_request = type("MockRequest", (object,), {})()
        mock_request.env = self.env(user=visitor)
        self.safe_patch(
            "odoo.addons.user_websites_seo.controllers.main.request",
            new=mock_request,
        )

        # Call the controller method
        response = self.controller.user_blog_index("seo-test-user")

        # Check that main_object is set
        self.assertIn("main_object", response.qcontext)

        main_obj = response.qcontext["main_object"]
        msg = (
            "main_object MUST be de-elevated to the requesting visitor's own "
            "env (ADR-0078), not left bound to whatever env it arrived with."
        )
        self.assertEqual(main_obj.env.uid, visitor.id, msg)
        self.assertNotEqual(
            main_obj.env.uid,
            self.regular_user.env.uid,
            "Test setup assumption broken: the visitor and the original env "
            "must actually differ, or this assertion proves nothing.",
        )
        self.assertEqual(main_obj, self.regular_user)
