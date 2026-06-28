# -*- coding: utf-8 -*-
import odoo.tests
from odoo.tests import tagged
import logging
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
        self.authenticate(None, None)
        create_url = f"/{self.user_a.website_slug}/create_blog"

        try:
            self.url_open(
                create_url,
                data={"csrf_token": odoo.http.Request.csrf_token(self)},
                method="POST",
            )
        except Exception as e:  # audit-ignore-catch-all
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
