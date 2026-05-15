# -*- coding: utf-8 -*-
import unittest
import odoo.tests
from odoo.addons.user_websites.hooks import post_init_hook


@odoo.tests.common.tagged("post_install", "-at_install")
class TestDocumentation(odoo.tests.common.HttpCase):

    def setUp(self):
        super(TestDocumentation, self).setUp()

        # Create a standard internal user with no special administrative privileges
        self.regular_user = self.env["res.users"].create(
            {
                "name": "Doc Reader",
                "login": "docreader",
                "email": "docreader@example.com",
                "group_ids": [(6, 0, [self.env.ref("base.group_portal").id])],
            }
        )

    def test_01_documentation_hook_file_read(self):
        """
        Explicitly verify that the post_init_hook correctly utilizes file_open
        to read the HTML documentation from the disk.
        """
        if "knowledge.article" not in self.env:
            raise unittest.SkipTest(
                "knowledge.article API is not installed. Skipping documentation hook test."
            )

        # Trigger the hook directly to ensure it runs in this transaction
        post_init_hook(self.env)

        article = self.env["knowledge.article"].search(
            [("name", "=", "User Websites Documentation")], limit=1
        )

        self.assertTrue(
            article,
            "Documentation article should be created dynamically via the API hook.",
        )

        self.assertIn(
            "Proxy Ownership Pattern",
            article.body,
            "The hook must successfully read the actual HTML file content.",
        )
        self.assertNotIn(
            "Error loading documentation file",
            article.body,
            "The file_open utility should not throw an exception.",
        )

    def test_02_documentation_route_authenticated(self):
        # [@ANCHOR: test_documentation_route]
        # Tests [@ANCHOR: controller_user_websites_documentation]
        """
        Verify that an authenticated user can access the documentation route,
        testing both the redirect (if API is present) and the fallback (if absent/unpublishable).
        """
        self.authenticate(self.regular_user.login, self.regular_user.login)
        response = self.url_open("/user-websites/documentation")

        if "knowledge.article" in self.env:
            article = self.env["knowledge.article"].search(
                [("name", "=", "User Websites Documentation")]
            )

            # Check if the article model has the website_url routing capability
            if hasattr(article, "website_url") and article.website_url:
                self.assertIn(
                    article.website_url.encode(),
                    response.url.encode(),
                    "Should redirect to the knowledge article URL.",
                )
            else:
                # If API is present but lacks frontend publishing, it MUST fallback to QWeb
                self.assertEqual(
                    response.status_code, 200, "Should fallback to 200 OK."
                )
                self.assertIn(
                    b"User Websites Module Documentation",
                    response.content,
                    "Should render fallback QWeb template.",
                )
        else:
            # API is entirely absent: MUST fallback to QWeb
            self.assertEqual(
                response.status_code,
                200,
                "Authenticated user should receive a 200 OK response.",
            )
            self.assertIn(
                b"User Websites Module Documentation",
                response.content,
                "The rendered page is missing the primary title.",
            )

    def test_03_documentation_route_unauthenticated(self):
        """
        Verify that an unauthenticated user is redirected to the login page
        since the route is strictly protected with auth="user".
        """
        self.authenticate(None, None)
        response = self.url_open("/user-websites/documentation")

        self.assertEqual(
            response.status_code,
            200,
            "The redirect to the login page should resolve successfully.",
        )
        self.assertIn(
            b"/web/login",
            response.url.encode(),
            "Unauthenticated guest users should be redirected to the login screen.",
        )

    def test_04_knowledge_api_absence_safety(self):
        """
        Explicitly test that if the API is absent, the system does not
        attempt to call it and gracefully defaults to the QWeb menu.
        """
        if "knowledge.article" in self.env:
            raise unittest.SkipTest(
                "knowledge.article API IS installed. Skipping API absence test."
            )

        self.authenticate(self.regular_user.login, self.regular_user.login)
        try:
            response = self.url_open("/user-websites/documentation")
            self.assertEqual(response.status_code, 200)
        except Exception as e:
            self.fail(f"Route failed unexpectedly when API is absent: {e}")
