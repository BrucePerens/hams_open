# This software is distributed under the terms of the Affero General Public License (AGPL-3).

# -*- coding: utf-8 -*-
from odoo.tests import tagged
from odoo.addons.zero_sudo.tests.real_transaction import RealTransactionCase
from odoo.exceptions import AccessError
from psycopg2 import IntegrityError
from odoo.tools import mute_logger


@tagged("post_install", "-at_install")
class TestORMSecurity(RealTransactionCase):
    """
    Tests focused on preventing malicious authenticated users from bypassing
    the controllers and exploiting the ORM/RPC layer.
    """

    def setUp(self):
        super(TestORMSecurity, self).setUp()

        self.user_a = self.env["res.users"].create(
            {
                "name": "Malice Attacker",
                "login": "malice",
                "email": "malice@example.com",
                "website_slug": "malice-attacker",
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
                "name": "Innocent Victim",
                "login": "victim",
                "email": "victim@example.com",
                "website_slug": "innocent-victim",
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

    def test_01_prevent_cross_user_page_creation(self):
        """
        Ensure a regular user cannot use the ORM to create a website.page
        and assign the ownership to another user.
        """
        # Malice tries to create a page but assigns owner_user_id to Victim
        with self.assertRaises(
            AccessError,
            msg="Record rules must block users from creating pages for other users.",
        ):
            self.env["website.page"].with_user(self.user_a).create(
                {
                    "url": f"/{self.user_b.website_slug}/hacked-page",
                    "name": "Hacked Page",
                    "type": "qweb",
                    "owner_user_id": self.user_b.id,
                }
            )
            self.env.flush_all()

    def test_02_prevent_report_state_tampering(self):
        """
        Ensure that while users can CREATE violation reports, they absolutely
        cannot WRITE to them to change their status (e.g., dismissing their own reports).
        """
        # Malice creates a report (allowed by perm_create=1)
        report = (
            self.env["content.violation.report"]
            .with_user(self.user_a)
            .create(
                {
                    "target_url": f"/{self.user_b.website_slug}/page",
                    "description": "Fake report to cause trouble.",
                    "reported_by_user_id": self.user_a.id,
                }
            )
        )

        # Malice tries to alter the report state or description (blocked by perm_write=0)
        with self.assertRaises(
            AccessError,
            msg="Users must not be able to edit violation reports after submission.",
        ):
            report.with_user(self.user_a).write({"state": "dismissed"})
            self.env.flush_all()

    @mute_logger("odoo.sql_db")
    def test_03_enforce_slug_uniqueness_db_level(self):
        """
        Verify the database-level SQL constraint `_website_slug_unique` prevents
        direct ORM injection of duplicate slugs, even if the compute method is bypassed.
        """
        # We attempt to force User B's slug to match User A's exactly, bypassing ORM checks.
        with self.assertRaises(
            IntegrityError, msg="The database must enforce unique website slugs."
        ):
            # We use an isolated cursor execution to catch the raw PSQL IntegrityError
            with self.env.cr.savepoint():
                self.env.cr.execute(
                    "UPDATE res_users SET website_slug = %s WHERE id = %s",
                    (self.user_a.website_slug, self.user_b.id),
                )

    def test_04_prevent_blog_post_theft_and_spoofing(self):
        """
        Ensure a user cannot create or edit a blog post pretending to be another user.
        """
        # 1. Test Spoofed Creation
        blog = self.env["blog.blog"].create({"name": "Test Blog"})

        # Malice tries to create a post, but sets Victim as the owner
        with self.assertRaises(
            AccessError,
            msg="Record rules must block users from creating blog posts for other users.",
        ):
            self.env["blog.post"].with_user(self.user_a).create(
                {
                    "name": "Spoofed Post",
                    "blog_id": blog.id,
                    "owner_user_id": self.user_b.id,
                }
            )
            self.env.flush_all()

        # 2. Test Stolen Ownership (Write)
        # Victim creates a legitimate post
        post = self.env["blog.post"].create(
            {
                "name": "Victim Post",
                "blog_id": blog.id,
                "owner_user_id": self.user_b.id,
                "is_published": True,
            }
        )

        # Malice attempts to seize ownership or edit it
        with self.assertRaises(
            AccessError,
            msg="Record rules must protect blog posts from unauthorized ORM writes.",
        ):
            post.with_user(self.user_a).write({"name": "Stolen Post"})
            self.env.flush_all()

    def test_05_other_user_can_read_published_content(self):
        """
        Verify that an 'Other User' has successful READ access to public content,
        satisfying the contextual success state of the Three-Persona rule.
        """
        blog = self.env["blog.blog"].create({"name": "Test Blog"})

        # Victim creates a legitimate, published post
        published_post = self.env["blog.post"].create(
            {
                "name": "Public Post",
                "blog_id": blog.id,
                "owner_user_id": self.user_b.id,
                "is_published": True,
            }
        )

        # Malice (Other User) attempts to read the post via ORM
        # This should NOT raise an AccessError because standard website_blog rules allow reading published posts.
        try:
            read_post = (
                self.env["blog.post"].with_user(self.user_a).browse(published_post.id)
            )
            # Accessing a field triggers the read
            post_name = read_post.name
        except AccessError:
            self.fail(
                "An Other User should be able to read published blog posts via the ORM."
            )

        self.assertEqual(post_name, "Public Post")

    def test_06_prevent_ownership_transfer(self):
        """
        Verify that a user cannot give away their own page to another user
        to bypass quota limits or frame them for bad content.
        """
        # Victim creates a legitimate page and post
        page = self.env["website.page"].create(
            {
                "url": f"/{self.user_b.website_slug}/transfer-test",
                "name": "Victim Page",
                "type": "qweb",
                "owner_user_id": self.user_b.id,
            }
        )

        blog = self.env["blog.blog"].create({"name": "Test Blog"})
        post = self.env["blog.post"].create(
            {"name": "Victim Post", "blog_id": blog.id, "owner_user_id": self.user_b.id}
        )

        # Victim attempts to maliciously transfer ownership to Malice
        with self.assertRaises(
            AccessError,
            msg="Users must not be able to transfer ownership of their pages.",
        ):
            page.with_user(self.user_b).write({"owner_user_id": self.user_a.id})
            self.env.flush_all()

        with self.assertRaises(
            AccessError,
            msg="Users must not be able to transfer ownership of their posts.",
        ):
            post.with_user(self.user_b).write({"owner_user_id": self.user_a.id})
            self.env.flush_all()

    def test_07_qweb_arch_sanitization(self):
        # [@ANCHOR: test_website_page_sanitize_arch]

        # Tests [@ANCHOR: website_page_sanitize_arch]
        """
        # Tests [@ANCHOR: website_page_sanitize_arch]
        Verify that script tags, iframes, and dangerous QWeb directives are actively stripped
        from the arch field during create and write for non-administrative users.
        """
        malicious_arch = """<t name="Test">
            <script>alert("XSS")</script>
            <iframe src="http://evil.com"></iframe>
            <div t-eval="request.env['res.users'].sudo().search([])" onmouseover="alert(1)"></div>
        </t>"""

        page = (
            self.env["website.page"]
            .with_user(self.user_a)
            .create(
                {
                    "url": f"/{self.user_a.website_slug}/xss",
                    "name": "XSS Test",
                    "type": "qweb",
                    "owner_user_id": self.user_a.id,
                    "arch": malicious_arch,
                }
            )
        )

        # The sanitizer should have stripped script, iframe, and neutralized t-eval and onmouseover
        self.assertNotIn(
            "<script>", page.arch, "The script tag must be completely removed."
        )
        self.assertNotIn(
            "<iframe>", page.arch, "The iframe tag must be completely removed."
        )
        self.assertIn(
            'data-blocked-t-eval="request.env',
            page.arch,
            "The t-eval directive must be neutralized.",
        )
        self.assertIn(
            'data-blocked-onmouseover="alert(1)"',
            page.arch,
            "The inline JS event must be neutralized.",
        )

        # Test write operation
        page.with_user(self.user_a).write(
            {"arch": '<t name="Test2"><script>console.log("XSS2")</script></t>'}
        )
        self.assertNotIn(
            "<script>", page.arch, "The script tag must be removed during write()."
        )
