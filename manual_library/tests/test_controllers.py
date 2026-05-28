# -*- coding: utf-8 -*-
from odoo.tests.common import tagged
from odoo.addons.zero_sudo.tests.common import HamsHttpCase
import odoo.http


@tagged("post_install", "-at_install")
class TestManualControllers(HamsHttpCase):

    def setUp(self):
        super(TestManualControllers, self).setUp()

        self.internal_user = self.env["res.users"].create(
            {
                "name": "Controller Tester",
                "login": "controllertester",
                "email": "tester@manual.com",
            }
        )
        self.internal_user.partner_id.company_id = self.env.company

        # Setup Test Content
        self.root_public = self.env["knowledge.article"].create(
            {
                "name": "Public Root API",
                "is_published": True,
                "internal_permission": "read",
            }
        )

        self.internal_draft = self.env["knowledge.article"].create(
            {
                "name": "Internal Team Draft",
                "is_published": False,
                "internal_permission": "read",
                "parent_id": self.root_public.id,
            }
        )

        self.private_admin_doc = self.env["knowledge.article"].create(
            {
                "name": "Admin Passwords",
                "is_published": False,
                "internal_permission": "none",
            }
        )

        self.shared_root_doc = self.env["knowledge.article"].create(
            {
                "name": "Shared Department Root",
                "is_published": False,
                "internal_permission": "none",
                "member_ids": [(4, self.internal_user.id)],
            }
        )

    def test_01_public_guest_routing(self):
        # [@ANCHOR: test_controller_manual_article_view]
        # Tests [@ANCHOR: controller_manual_article_view]
        """Public guests can hit the root manual route and see published content."""
        self.authenticate(None, None)
        response = self.url_open("/manual")
        self.assertEqual(
            response.status_code, 200, "Base route should render successfully."
        )
        self.assertIn(
            b"Public Root API",
            response.content,
            "Published root article should be visible.",
        )
        self.assertNotIn(
            b"Internal Team Draft",
            response.content,
            "Unpublished children must be hidden from sidebar.",
        )

        # Attempt to access unpublished internal draft directly via URL
        response_draft = self.url_open(self.internal_draft.website_url)
        self.assertEqual(
            response_draft.status_code,
            404,
            "Controller must suppress AccessError and return a clean 404 for unauthorized guests.",
        )

    def test_02_internal_user_routing(self):
        """Internal users authenticated in the session can see internal drafts."""
        self.authenticate(self.internal_user.login, self.internal_user.login)

        # Access the internal draft
        response = self.url_open(self.internal_draft.website_url)
        self.assertEqual(
            response.status_code, 200, "Internal user can route to workspace draft."
        )
        self.assertIn(b"Internal Team Draft", response.content)

        # Ensure Private/None permission articles throw 404 to hide their existence
        response_private = self.url_open(self.private_admin_doc.website_url)
        self.assertEqual(
            response_private.status_code,
            404,
            "Private articles must return 404 to internal users lacking permissions.",
        )

    def test_03_base_route_fallback(self):
        """If no article is specified, the controller falls back to the first available root."""
        # Must explicitly clear the internal_user session from test_02 to run as a public guest
        self.authenticate(None, None)

        # Unpublish ALL root articles to ensure a true empty state,
        # protecting the test against data injected by other modules (like user_websites).
        all_roots = self.env["knowledge.article"].search([("parent_id", "=", False)])
        all_roots.write({"is_published": False})

        # A guest hitting the base route with no public root articles available
        # should gracefully receive a 404 rather than an IndexError or bypassing rules.
        response = self.url_open("/manual")
        self.assertEqual(
            response.status_code,
            404,
            "Graceful 404 if no articles are available to render.",
        )

    def test_04_feedback_anti_spam(self):
        """Verify that bots triggering the honeypot field are silently rejected without incrementing counts."""
        self.authenticate(None, None)

        initial_helpful = self.root_public.helpful_count

        response = self.url_open(
            "/manual/feedback",
            data={
                "csrf_token": odoo.http.Request.csrf_token(self),
                "article_id": self.root_public.id,
                "is_helpful": "1",
                "website_feedback_honeypot": "Im a bot!",
            },
            method="POST",
        )

        self.assertEqual(
            response.status_code, 200
        )  # URL open follows silent redirects, yielding 200

        # Count must not have changed
        self.root_public.invalidate_recordset(["helpful_count"])
        self.assertEqual(
            self.root_public.helpful_count,
            initial_helpful,
            "Honeypot must block mutation.",
        )

    def test_05_dynamic_sidebar_categorization(self):
        """Verify the controller groups articles properly based on member_ids and permissions."""
        self.authenticate(self.internal_user.login, self.internal_user.login)

        response = self.url_open("/manual")
        self.assertEqual(response.status_code, 200)

        # We expect the QWeb to render "WORKSPACE" for root_public
        self.assertIn(b"Workspace", response.content)
        # We expect the QWeb to render "SHARED" for shared_root_doc
        self.assertIn(b"Shared", response.content)
        self.assertIn(b"Shared Department Root", response.content)
        # We expect the QWeb to NOT render "Private" since the user owns no private roots
        self.assertNotIn(b"Admin Passwords", response.content)

    def test_06_manual_templates_rendering(self):
        # [@ANCHOR: test_manual_templates_rendering]
        self.authenticate(None, None)
        # Tests [@ANCHOR: controller_manual_article_view]
        # Tests [@ANCHOR: story_article_view]
        # Tests [@ANCHOR: journey_user_browsing]
        self.url_open("/manual")
