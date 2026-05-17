# -*- coding: utf-8 -*-
import odoo.tests
import logging

_logger = logging.getLogger(__name__)

@odoo.tests.common.tagged("post_install", "-at_install")
class TestAdvancedEdgeCases(odoo.tests.common.HttpCase):
    def setUp(self):
        super(TestAdvancedEdgeCases, self).setUp()

        self.user_empty = self.env["res.users"].create(
            {
                "name": "Empty Blog User",
                "login": "emptyuser",
                "email": "empty@example.com",
                "website_slug": "emptyuser",
                "group_ids": [
                    (
                        6,
                        0,
                        [
                            self.env.ref("base.group_portal").id,
                            self.env.ref("user_websites.group_user_websites_user").id,
                        ],
                    )
                ],
            }
        )

        self.doomed_group = self.env["user.websites.group"].create(
            {
                "name": "Doomed Group",
                "website_slug": "doomed-group",
            }
        )

    def test_01_group_deletion_cascade(self):
        """
        Verify that deleting a User Websites Group correctly cascades and
        destroys the linked website.page and blog.post records to prevent ghost data.
        """
        # Create a page and post tied to the group
        page = self.env["website.page"].create(
            {
                "url": f"/{self.doomed_group.website_slug}/home",
                "name": "Group Home",
                "type": "qweb",
                "user_websites_group_id": self.doomed_group.id,
            }
        )

        blog = self.env["blog.blog"].search([("name", "=", "Community Blog")], limit=1)
        if not blog:
            blog = self.env["blog.blog"].create({"name": "Community Blog"})

        post = self.env["blog.post"].create(
            {
                "name": "Group Post",
                "blog_id": blog.id,
                "user_websites_group_id": self.doomed_group.id,
            }
        )

        # Ensure they exist
        self.assertTrue(page.exists())
        self.assertTrue(post.exists())

        # Delete the group
        self.doomed_group.unlink()

        # Check that the cascade destroyed the content
        self.assertFalse(
            page.exists(),
            "The linked website.page should be deleted when the group is deleted.",
        )
        self.assertFalse(
            post.exists(),
            "The linked blog.post should be deleted when the group is deleted.",
        )

    def test_02_empty_blog_pager_rendering(self):
        """
        Ensure that visiting a private blog route when the user has 0 posts
        does not crash the custom pager injection.
        """
        # Ensure the user has absolutely no posts
        posts = self.env["blog.post"].search(
            [("owner_user_id", "=", self.user_empty.id)]
        )
        self.assertEqual(len(posts), 0)

        # Access the blog route
        response = self.url_open(f"/{self.user_empty.website_slug}/blog")

        # It should render successfully (HTTP 200) without throwing a 500 error
        self.assertEqual(response.status_code, 200)

    def test_03_report_violation_missing_referrer(self):
        """
        Ensure the report submission form safely redirects to '/' if the
        HTTP Referrer header is missing or stripped by the browser.
        """
        self.authenticate(None, None)

        # We manually construct a request with NO headers to simulate a stripped Referrer
        response = self.url_open(
            "/website/report_violation",
            data={
                "csrf_token": odoo.http.Request.csrf_token(self),
                "url": "/some/test/url",
                "description": "Testing stripped referrer",
                "email": "ghost@example.com",
            },
            headers={},
            method="POST",
        )

        # The controller should catch the missing referrer and redirect to /?report_submitted=1
        self.assertEqual(response.status_code, 200)

        report = self.env["content.violation.report"].search(
            [("reported_by_email", "=", "ghost@example.com")], limit=1
        )
        self.assertTrue(
            report, "Report should still be created despite missing referrer."
        )

    def test_04_multi_website_routing_isolation(self):
        """
        Ensure that when a site is created, it is bound to the current website
        and does not bleed into other websites in a multi-website environment.
        """
        admin_uid = self.env.ref("base.user_admin").id
        Website = self.env["website"].with_user(admin_uid)
        website_a = Website.get_current_website()

        # Simulate a secondary website environment
        Website.create({"name": "Secondary Website", "domain": "odoo:8070"})

        self.authenticate(self.user_empty.login, self.user_empty.login)

        # Create site while on Website A
        self.url_open(
            f"/{self.user_empty.website_slug}/create_site",
            data={"csrf_token": odoo.http.Request.csrf_token(self)},
            method="POST",
        )

        created_page = self.env["website.page"].search(
            [("url", "=", f"/{self.user_empty.website_slug}/home")]
        )

        self.assertEqual(
            created_page.website_id.id,
            website_a.id,
            "The generated page must be explicitly bound to the website where it was created.",
        )

    def test_05_website_page_creation_rpc_context(self):
        """
        Simulate website_page creation outside of a standard HTTP request (e.g. XML-RPC).
        This verifies that get_current_website() doesn't execute an unrestricted search
        that violates ACLs or crashes when request.website is absent.
        """
        # Create a new environment without an HTTP request context
        env_no_request = self.env(context={})

        try:
            page = (
                env_no_request["website.page"]
                .with_user(self.user_empty)
                .create(
                    {
                        "url": f"/{self.user_empty.website_slug}/rpc-test",
                        "name": "RPC Page",
                        "type": "qweb",
                        "owner_user_id": self.user_empty.id,
                    }
                )
            )
            self.assertTrue(
                page.id,
                "Page should be successfully created even without an active HTTP request.",
            )
        except Exception as e:
            _logger.error("website.page creation failed in RPC context: %s", e)
            self.fail(f"website.page creation failed in RPC context: {e}")
