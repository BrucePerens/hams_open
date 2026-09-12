# -*- coding: utf-8 -*-
# Copyright © Bruce Perens K6BP.
# SPDX-License-Identifier: AGPL-3.0-or-later
import odoo.tests
from odoo.tests import tagged
from odoo.addons.zero_sudo.tests.common import HamsHttpCase
import logging
import urllib.error
import uuid

_logger = logging.getLogger(__name__)


@tagged("post_install", "-at_install")
class TestAdvancedEdgeCases(HamsHttpCase):
    def setUp(self):
        super(TestAdvancedEdgeCases, self).setUp()

        unique_id = str(uuid.uuid4())[:8]
        self.user_empty = self.env["res.users"].create(
            {
                "name": f"Empty Blog User {unique_id}",
                "login": f"emptyuser_{unique_id}",
                "email": f"empty_{unique_id}@example.com",
                "website_slug": f"emptyuser_{unique_id}",
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
                "name": f"Doomed Group {unique_id}",
                "website_slug": f"doomed-group-{unique_id}",
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
            [("owner_user_id", "=", self.user_empty.id)], limit=1
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
            "/website/report_violation",  # burn-ignore-route
            data={
                "csrf_token": odoo.http.Request.csrf_token(self),
                "url": "/some/test/url",
                "reason": "Harassment or bullying",
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
        svc_uid = self.env["zero_sudo.security.utils"]._get_service_uid(
            "user_websites.user_websites_service_account"
        )
        Website = self.env["website"].with_user(svc_uid)
        website_a = Website.get_current_website()

        # Simulate a secondary website environment. This is test fixture
        # setup, not production behavior under test, so it's created with
        # the ordinary (privileged) test-runner user rather than svc_uid --
        # the service account's ACL on website is deliberately read-only
        # (access_website_svc), and it has no ACL on res.lang either, so
        # a real Website.create() call under svc_uid would fail on the
        # res.lang default_get lookup before even reaching that create
        # permission check.
        self.env["website"].create({"name": "Secondary Website", "domain": "odoo:8070"})

        self.authenticate(self.user_empty.login, self.user_empty.login)

        # Create site while on Website A
        self.url_open(
            f"/{self.user_empty.website_slug}/create_site",
            data={"csrf_token": odoo.http.Request.csrf_token(self)},
            method="POST",
        )

        created_page = self.env["website.page"].search(
            [("url", "=", f"/{self.user_empty.website_slug}/home")], limit=1
        )

        self.assertEqual(
            created_page.website_id.id,
            website_a.id,
            "The generated page must be explicitly bound to the website where it was created.",
        )

    def test_04b_blog_blog_published_read_rule_excludes_inactive(self):
        """
        Bug-hunt fix (docs/bug_hunt_claims/.../published_read_rules.md):
        blog_blog_published_read_rule's own domain used to be the
        unconditional [(1, '=', 1)], which OR-defeated stock Odoo's own
        website_blog_public rule ([('active', '=', True)], same model, same
        base.group_public/base.group_portal groups). Odoo OR-combines
        same-model group-scoped rule domains, so the combined domain
        optimized to the broader (1,'=',1) -- any public or portal caller
        could read every blog.blog record regardless of its active flag,
        including archived ones stock's own rule exists specifically to
        hide. active_test=False bypasses the ORM's own automatic active-field
        filter so this test isolates the ir.rule's own domain as the only
        thing standing between the caller and the archived record.
        """
        active_blog = self.env["blog.blog"].create({"name": "Active Blog For Read Rule Test"})
        inactive_blog = self.env["blog.blog"].create(
            {"name": "Archived Blog For Read Rule Test", "active": False}
        )

        readable_ids = (
            self.env["blog.blog"]
            .with_user(self.user_empty)
            .with_context(active_test=False)
            .search([("id", "in", [active_blog.id, inactive_blog.id])])
            .ids
        )
        self.assertEqual(
            readable_ids,
            [active_blog.id],
            "A portal user must not be able to read an archived (active=False) "
            "blog.blog record -- if the archived blog is included, "
            "blog_blog_published_read_rule regressed and OR-defeats stock "
            "Odoo's own active-only visibility rule again.",
        )

    def test_04c_blog_post_published_read_rule_scopes_by_website(self):
        """
        Bug-hunt fix (docs/bug_hunt_claims/.../published_read_rules.md):
        blog_post_published_read_rule's own domain used to filter on
        is_published rather than website_published (a computed field:
        is_published AND (no website_id OR website_id matches the current
        website)) -- OR-defeating stock Odoo's own website_blog_post_public
        rule, which uses website_published. In a multi-website deployment (a
        real, provisioned configuration here: the service account holds
        website.group_multi_website), a blog.post published on one website
        became readable by a visitor whose request context named a different
        one. Prove a post explicitly bound to Website A is not readable by a
        portal user whose context names Website B, but remains readable from
        Website A itself.
        """
        svc_uid = self.env["zero_sudo.security.utils"]._get_service_uid(
            "user_websites.user_websites_service_account"
        )
        website_a = self.env["website"].with_user(svc_uid).get_current_website()
        website_b = self.env["website"].create(
            {"name": "Secondary Website For Read Rule Test", "domain": "odoo:8071"}
        )

        # blog.post's own website_id is a related field
        # (related='blog_id.website_id', readonly=True, store=True, per
        # stock website_blog/models/website_blog.py) -- it is derived from
        # the PARENT blog, not independently settable on the post itself.
        # A new blog (not the shared "Community Blog" other tests use) is
        # created here specifically bound to website_a.
        blog_on_a = self.env["blog.blog"].create(
            {"name": "Website A Blog For Read Rule Test", "website_id": website_a.id}
        )

        post = self.env["blog.post"].create(
            {
                "name": "Website A Only Post For Read Rule Test",
                "blog_id": blog_on_a.id,
                "is_published": True,
                "owner_user_id": self.env.user.id,
            }
        )
        post.invalidate_recordset()

        self.assertNotEqual(
            website_a.id, website_b.id, "fixture: two distinct websites required"
        )
        self.assertEqual(
            post.website_id.id,
            website_a.id,
            "fixture: post.website_id must land on website_a",
        )
        self.assertNotEqual(
            post.owner_user_id.id,
            self.user_empty.id,
            "fixture: owner_user_id must NOT be user_empty's own id, or "
            "blog_post_private_rule (a DIFFERENT rule, unconditional on "
            "ownership) would let user_empty read this post regardless of "
            "website scoping, masking what this test is trying to isolate",
        )

        readable_from_b = (
            self.env["blog.post"]
            .with_user(self.user_empty)
            .with_context(website_id=website_b.id)
            .search([("id", "=", post.id)])
            .ids
        )
        self.assertEqual(
            readable_from_b,
            [],
            "A blog.post explicitly bound to one website must not be readable "
            "by a portal user whose request context names a different website "
            "-- if it is, blog_post_published_read_rule regressed and "
            "OR-defeats stock Odoo's own website-scoped visibility rule again.",
        )

        readable_from_a = (
            self.env["blog.post"]
            .with_user(self.user_empty)
            .with_context(website_id=website_a.id)
            .search([("id", "=", post.id)])
            .ids
        )
        self.assertEqual(
            readable_from_a,
            [post.id],
            "The same post must remain readable when the request context "
            "names the website it's actually bound to.",
        )

    def test_05_website_page_creation_rpc_context(self):
        """
        Simulate website_page creation outside of a standard HTTP request (e.g. XML-RPC).
        This verifies that get_current_website() doesn't execute an unrestricted search
        that violates ACLs or crashes when request.website is absent.
        """
        # Create a new environment without an HTTP request context.
        # odoo.api.Environment has no with_context() of its own (that's a
        # recordset method) -- calling the environment directly with a new
        # context dict is the equivalent at the environment level.
        env_no_request = self.env(context=dict(self.env.context, website_id=False))

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
        except urllib.error.HTTPError as e:
            _logger.error("website.page creation failed in RPC context: %s", e)
            self.fail(f"website.page creation failed in RPC context: {e}")
