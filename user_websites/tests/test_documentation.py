# -*- coding: utf-8 -*-
import logging
from odoo.tests import tagged
from odoo.addons.zero_sudo.tests.real_transaction import RealTransactionCase
from odoo.addons.user_websites.hooks import post_init_hook

_logger = logging.getLogger(__name__)


@tagged("post_install", "-at_install")
class TestDocumentation(RealTransactionCase):

    def setUp(self):
        super(TestDocumentation, self).setUp()
        self.regular_user = None

        # Create a standard internal user with no special administrative privileges
        self.regular_user = self.env["res.users"].create(
            {
                "name": "Doc Reader",
                "login": "docreader",
                "email": "docreader@example.com",
                "group_ids": [(6, 0, [self.env.ref("base.group_portal").id])],
            }
        )
        self.env.cr.commit()

    def tearDown(self):
        # Resilient manual cleanup
        for attempt in range(5):
            try:
                with self.env.cr.savepoint():
                    if self.regular_user and self.regular_user.exists():
                        self.regular_user.unlink()
                break
            except Exception as e:  # audit-ignore-catch-all
                _logger.warning("Resilient cleanup encountered exception: %s", e)

        self.env.cr.commit()
        super(TestDocumentation, self).tearDown()

    def test_01_documentation_hook_file_read(self):
        # Tests [@ANCHOR: documentation_bootstrap]
        """
        Explicitly verify that the documentation bootstrap mechanism (via _register_hook
        and post_init_hook) correctly utilizes file_open to read the HTML
        documentation from the disk.
        """
        # Trigger the hook directly to ensure it runs in this transaction
        post_init_hook(self.env)
        self.env.cr.commit()

        article = self.env["knowledge.article"].search(
            [("name", "=", "User Websites Documentation")], limit=1
        )

        self.assertTrue(
            article,
            "Documentation article should be created dynamically via the API hook.",
        )

        self.assertIn(
            "Creating Your Site",
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

        # Sync cursor state
        self.env.cr.commit()

        article = self.env["knowledge.article"].search(
            [("name", "=", "User Websites Documentation")]
        )

        # The knowledge.article model natively implements website_url
        website_url = article.website_url

        if website_url:
            self.assertIn(
                website_url.encode(),
                response.url.encode(),
                "Should redirect to the knowledge article URL.",
            )
        else:
            # If API is present but lacks frontend publishing, it MUST fallback to QWeb
            self.assertEqual(response.status_code, 200, "Should fallback to 200 OK.")
            self.assertIn(
                b"User Websites Module Documentation",
                response.content,
                "Should render fallback QWeb template.",
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
            b"/web/login",  # burn-ignore-route
            response.url.encode(),
            "Unauthenticated guest users should be redirected to the login screen.",
        )
