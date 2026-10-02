# Copyright © Bruce Perens K6BP.
# SPDX-License-Identifier: AGPL-3.0-or-later

# -*- coding: utf-8 -*-
import odoo.tests
from odoo.tests import tagged
import logging
import urllib.error
import uuid

_logger = logging.getLogger(__name__)


@tagged("post_install", "-at_install")
class TestBlogPostOwnership(odoo.tests.common.HttpCase):
    def setUp(self):
        super(TestBlogPostOwnership, self).setUp()

        main_website = self.env["website"].get_current_website()
        if not main_website:
            main_website = self.env["website"].search([], limit=1)

        unique_id = str(uuid.uuid4())[:8]
        self.user_a = self.env["res.users"].create(
            {
                "name": f"User A {unique_id}",
                "login": f"usera_{unique_id}",
                "password": "usera",
                "email": f"usera_{unique_id}@example.com",
                "website_slug": f"usera_{unique_id}",
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

        self.user_b = self.env["res.users"].create(
            {
                "name": f"User B {unique_id}",
                "login": f"userb_{unique_id}",
                "password": "userb",
                "email": f"userb_{unique_id}@example.com",
                "website_slug": f"userb_{unique_id}",
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

        self.blog = self.env["blog.blog"].create(
            {
                "name": f"{self.user_a.name}'s Blog",
                "website_id": main_website.id,
                "owner_user_id": self.user_a.id,
            }
        )

        self.post_a = self.env["blog.post"].create(
            {
                "name": "User A Post",
                "blog_id": self.blog.id,
                "is_published": True,
                "website_id": main_website.id,
                "owner_user_id": self.user_a.id,
            }
        )

    def test_01_user_blog_route_isolation(self):
        url_a = f"/{self.user_a.website_slug}/blog"
        response = self.url_open(url_a)

        self.assertEqual(response.status_code, 200)
        self.assertIn(
            b"User A Post", response.content, "User A's blog should show User A's post"
        )

    def test_02_user_b_cannot_claim_user_a_post(self):
        url_b = f"/{self.user_b.website_slug}/blog"
        response = self.url_open(url_b)

        self.assertEqual(response.status_code, 200)
        self.assertNotIn(
            b"User A Post",
            response.content,
            "User B's blog should NOT show User A's post",
        )

    def test_03_report_button_visibility(self):
        url_a_blog = f"/{self.user_a.website_slug}/blog"
        report_button_text = b"Report Violation"

        self.authenticate(self.user_a.login, self.user_a.login)
        response = self.url_open(url_a_blog)
        if response.status_code == 404:
            _logger.error(f"TEST 03 404 RESPONSE: {response.text[:500]}")
        self.assertEqual(response.status_code, 200)

        self.assertNotIn(
            report_button_text,
            response.content,
            "Content owner (User A) should NOT see the 'Report Violation' button on their own blog.",
        )

        self.authenticate(self.user_b.login, self.user_b.login)
        response = self.url_open(url_a_blog)
        self.assertEqual(response.status_code, 200)
        self.assertIn(
            report_button_text,
            response.content,
            "Visitor (User B) SHOULD see the 'Report Violation' button on User A's blog.",
        )

    def test_04_public_cannot_create_blog(self):
        # Tests [@ANCHOR: user_websites:COMM_create_blog]
        self.authenticate(None, None)
        create_url = f"/{self.user_a.website_slug}/create_blog"

        try:
            self.url_open(
                create_url,
                data={"csrf_token": odoo.http.Request.csrf_token(self)},
                method="POST",
            )
        except urllib.error.HTTPError as e:
            _logger.info("Expected error on public blog creation: %s", e)

        public_created_post = self.env["blog.post"].search(
            [
                ("owner_user_id", "=", self.user_a.id),
                ("name", "=", "Welcome to my Blog"),
            ]
        )
        self.assertFalse(
            public_created_post,
            "Public user should not be able to trigger blog creation",
        )

    def test_05_owner_can_create_blog_post(self):
        # Tests [@ANCHOR: user_websites:COMM_create_blog_post]
        # Real gap found live 2026-09-29: blog_index's own template offered no way at all to add a
        # post to an existing, empty blog, even for the owner (see create_blog_post()'s own
        # comment). self.blog/self.post_a already exist for user_a from setUp, so this proves the
        # route works for an owner who already has content too, not just a first-post case.
        self.authenticate(self.user_a.login, self.user_a.login)
        before_count = self.env["blog.post"].search_count([("blog_id", "=", self.blog.id)])

        response = self.url_open(
            f"/{self.user_a.website_slug}/create_blog_post",
            data={"csrf_token": odoo.http.Request.csrf_token(self)},
            method="POST",
        )
        self.assertEqual(response.status_code, 200)

        new_posts = self.env["blog.post"].search(
            [("blog_id", "=", self.blog.id), ("name", "=", "New Post")]
        )
        self.assertTrue(new_posts, "create_blog_post() should have created a new post in the owner's own blog.")
        self.assertEqual(new_posts.owner_user_id.id, self.user_a.id)
        self.assertEqual(
            new_posts.author_id.id,
            self.user_a.partner_id.id,
            "The byline should credit the real owner, not website_blog's own default "
            "(self.env.user.partner_id -- the elevated service account this create() runs as).",
        )
        self.assertEqual(
            self.env["blog.post"].search_count([("blog_id", "=", self.blog.id)]),
            before_count + 1,
        )
        # night_shift_todo/high/personal-blog-owner-cannot-actually-edit-or-publish-their-own-
        # new-post-0c44e7eb.md: this redirect used to go straight into website_blog's own
        # generic post-view route, relying on a generic website-builder editor toolbar that
        # never actually renders for this module's real target user (see
        # blog_post_edit()'s own comment, controllers/main.py). It now redirects into this
        # module's own dedicated, ownership-scoped edit form instead, so the owner lands
        # somewhere they can actually title, write and publish the post they just created.
        self.assertTrue(
            response.url.rstrip("/").endswith(f"/blog_post/edit/{new_posts.id}"),
            f"Expected the final URL to resolve to the new post's own edit form, got: {response.url}",
        )
        self.assertIn(b"Edit Blog Post", response.content)

    def test_06_non_owner_cannot_create_blog_post_on_someone_elses_slug(self):
        # Tests [@ANCHOR: user_websites:COMM_create_blog_post]
        self.authenticate(self.user_b.login, self.user_b.login)
        before_count = self.env["blog.post"].search_count([("blog_id", "=", self.blog.id)])

        try:
            self.url_open(
                f"/{self.user_a.website_slug}/create_blog_post",
                data={"csrf_token": odoo.http.Request.csrf_token(self)},
                method="POST",
            )
        except urllib.error.HTTPError as e:
            _logger.info("Expected error on cross-account blog post creation: %s", e)

        self.assertEqual(
            self.env["blog.post"].search_count([("blog_id", "=", self.blog.id)]),
            before_count,
            "User B must not be able to create a post in User A's blog via User A's own slug.",
        )

    def test_07_owner_can_edit_own_blog_post(self):
        # [@ANCHOR: test_owner_can_edit_own_blog_post]

        # Tests [@ANCHOR: user_websites:COMM_blog_post_edit]

        # Tests [@ANCHOR: user_websites:COMM_blog_post_edit_submit]

        # Tests [@ANCHOR: user_websites:UX_BLOG_POST_EDIT_FORM]
        """The actual fix for night_shift_todo/high/
        personal-blog-owner-cannot-actually-edit-or-publish-their-own-new-post-0c44e7eb.md: a
        real, dedicated edit form the owner can reach without the generic website-builder
        editor toolbar (which never renders for this persona -- see the parent to-do)."""
        self.authenticate(self.user_a.login, self.user_a.login)

        get_response = self.url_open(f"/blog_post/edit/{self.post_a.id}")
        self.assertEqual(get_response.status_code, 200)
        self.assertIn(b"User A Post", get_response.content)

        post_response = self.url_open(
            f"/blog_post/edit/submit/{self.post_a.id}",
            data={
                "csrf_token": odoo.http.Request.csrf_token(self),
                "name": "User A Post, Retitled",
                "content": "<p>Real body content the owner just wrote.</p>",
                "is_published": "on",
            },
            method="POST",
        )
        self.assertEqual(post_response.status_code, 200)

        self.post_a.invalidate_recordset()
        self.assertEqual(self.post_a.name, "User A Post, Retitled")
        self.assertIn("Real body content", self.post_a.content or "")
        self.assertTrue(self.post_a.is_published)
        self.assertTrue(
            post_response.url.rstrip("/").endswith(f"-{self.post_a.id}"),
            f"Expected the final URL to land on the published post itself, got: {post_response.url}",
        )

    def test_08_non_owner_cannot_edit_someone_elses_blog_post(self):
        # [@ANCHOR: test_blog_post_edit_denied_for_non_owner]

        # Tests [@ANCHOR: user_websites:COMM_blog_post_edit]

        # Tests [@ANCHOR: user_websites:COMM_blog_post_edit_submit]

        # Tests [@ANCHOR: user_websites:COMM_get_own_blog_post_for_edit]
        self.authenticate(self.user_b.login, self.user_b.login)
        original_name = self.post_a.name

        get_response = self.url_open(f"/blog_post/edit/{self.post_a.id}")
        self.assertEqual(get_response.status_code, 200)
        self.assertNotIn(b"blog_post_edit_form", get_response.content)

        self.url_open(
            f"/blog_post/edit/submit/{self.post_a.id}",
            data={
                "csrf_token": odoo.http.Request.csrf_token(self),
                "name": "Hijacked Title",
                "content": "hijacked",
                "is_published": "on",
            },
            method="POST",
        )

        self.post_a.invalidate_recordset()
        self.assertEqual(
            self.post_a.name,
            original_name,
            "User B must not be able to edit User A's blog post.",
        )

    def test_09_group_member_can_edit_group_owned_blog_post(self):
        # [@ANCHOR: test_blog_post_edit_denied_for_non_member]

        # Tests [@ANCHOR: user_websites:COMM_blog_post_edit]

        # Tests [@ANCHOR: user_websites:COMM_blog_post_edit_submit]

        # Tests [@ANCHOR: user_websites:COMM_get_own_blog_post_for_edit]
        """A group blog post's edit access follows group membership, not just owner_user_id --
        the same two facts blog_post.py's own check_access() already enforces for write()."""
        unique_id = str(uuid.uuid4())[:8]
        website = self.env["website"].get_current_website() or self.env["website"].search(
            [], limit=1
        )
        group = self.env["user.websites.group"].create(
            {"name": f"Edit Test Group {unique_id}", "website_slug": f"edit-grp-{unique_id}"}
        )
        member = self.env["res.users"].create(
            {
                "name": f"Group Member {unique_id}",
                "login": f"groupmember_{unique_id}",
                "password": "groupmember",
                "email": f"groupmember_{unique_id}@example.com",
                "website_slug": f"groupmember_{unique_id}",
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
        group.write({"member_ids": [(4, member.id)]})
        group_blog = self.env["blog.blog"].create(
            {
                "name": f"{group.name}'s Blog",
                "website_id": website.id,
                "user_websites_group_id": group.id,
            }
        )
        group_post = self.env["blog.post"].create(
            {
                "name": "Group Post",
                "blog_id": group_blog.id,
                "website_id": website.id,
                "user_websites_group_id": group.id,
                "author_id": member.partner_id.id,
            }
        )

        self.authenticate(member.login, "groupmember")
        get_response = self.url_open(f"/blog_post/edit/{group_post.id}")
        self.assertEqual(get_response.status_code, 200)
        self.assertIn(b"Group Post", get_response.content)

        self.url_open(
            f"/blog_post/edit/submit/{group_post.id}",
            data={
                "csrf_token": odoo.http.Request.csrf_token(self),
                "name": "Group Post, Edited By Member",
                "content": "member-written body",
                "is_published": "on",
            },
            method="POST",
        )
        group_post.invalidate_recordset()
        self.assertEqual(group_post.name, "Group Post, Edited By Member")
        self.assertTrue(group_post.is_published)

        # A non-member must not be able to reach it.
        self.authenticate(self.user_b.login, self.user_b.login)
        self.url_open(
            f"/blog_post/edit/submit/{group_post.id}",
            data={
                "csrf_token": odoo.http.Request.csrf_token(self),
                "name": "Non-Member Hijack",
                "content": "hijacked",
                "is_published": "on",
            },
            method="POST",
        )
        group_post.invalidate_recordset()
        self.assertEqual(
            group_post.name,
            "Group Post, Edited By Member",
            "A non-member of the owning group must not be able to edit its post.",
        )

    def test_10_blog_post_edit_submit_sanitizes_content(self):
        # [@ANCHOR: test_blog_post_edit_submit_sanitizes_content]

        # Tests [@ANCHOR: user_websites:COMM_blog_post_edit_submit]
        """blog.post's own "content" field is sanitize=False (stock website_blog, kept that
        way so the generic website-builder snippet editor can write rich markup) -- a raw
        <script> submitted through this route's plain <textarea> must still never reach a
        published post's own content unsanitized, or it would be real, persistent stored XSS
        against every visitor of this post, not just the owner who wrote it."""
        self.authenticate(self.user_a.login, self.user_a.login)

        self.url_open(
            f"/blog_post/edit/submit/{self.post_a.id}",
            data={
                "csrf_token": odoo.http.Request.csrf_token(self),
                "name": "User A Post",
                "content": "<p>Safe text</p><script>alert('xss')</script>",
                "is_published": "on",
            },
            method="POST",
        )

        self.post_a.invalidate_recordset()
        self.assertIn("Safe text", self.post_a.content or "")
        self.assertNotIn("<script", self.post_a.content or "")
        self.assertNotIn("alert(", self.post_a.content or "")
