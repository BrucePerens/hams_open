# This software is distributed under the terms of the Affero General Public License (AGPL-3).

# -*- coding: utf-8 -*-
import odoo
import time
from odoo.tests import tagged
from odoo.addons.zero_sudo.tests.real_transaction import RealTransactionCase
import logging
import urllib.error
from odoo.exceptions import ValidationError

_logger = logging.getLogger(__name__)


@tagged("post_install", "-at_install")
class TestLifecycleAndGroups(RealTransactionCase):
    def setUp(self):
        super(TestLifecycleAndGroups, self).setUp()

        self.website = self.env["website"].get_current_website()
        if not self.website:
            self.website = self.env["website"].search([], limit=1)

        self.user_a = self.env["res.users"].create(
            {
                "name": "Alice Life",
                "login": "alicelife",
                "password": "alicelife",
                "email": "alice@example.com",
                "website_slug": "alicelife",
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

        self.test_group = self.env["user.websites.group"].create(
            {
                "name": "Test Group Site",
                "website_slug": "test-group-site",
            }
        )
        self.env.cr.commit()

    def test_01_group_creation_and_slug(self):
        self.assertEqual(
            self.test_group.website_slug,
            "test-group-site",
            "Slug should be saved correctly",
        )
        self.assertTrue(
            self.test_group.odoo_group_id, "Odoo security group should be auto-created"
        )
        self.assertEqual(
            self.test_group.odoo_group_id.name,
            "Website Group: Test Group Site",
            "Odoo group name mismatch",
        )

    def test_02_group_member_access(self):
        # [@ANCHOR: test_group_site_creation]
        # [@ANCHOR: test_group_site_routing]
        # Tests [@ANCHOR: UX_CREATE_SITE]
        # Tests [@ANCHOR: controller_user_websites_home]
        self.test_group.write({"member_ids": [(4, self.user_a.id)]})

        self.authenticate(self.user_a.login, self.user_a.login)

        create_url = f"/{self.test_group.website_slug}/create_site"

        self.env.cr.commit()
        response = self.url_open(
            create_url,
            data={"csrf_token": odoo.http.Request.csrf_token(self)},
            method="POST",
        )
        self.assertEqual(
            response.status_code,
            200,
            "Group member should be able to create group site",
        )

        self.env.cr.commit()

        group_home = self.env["website.page"].search(
            [
                ("url", "=", f"/{self.test_group.website_slug}/home"),
                ("user_websites_group_id", "=", self.test_group.id),
            ]
        )
        self.assertTrue(group_home, "Group homepage should exist after creation")

    def test_03_non_member_cannot_create_group_site(self):
        self.authenticate(self.user_a.login, self.user_a.login)

        create_url = f"/{self.test_group.website_slug}/create_site"

        try:
            self.env.cr.commit()
            self.url_open(
                create_url,
                data={"csrf_token": odoo.http.Request.csrf_token(self)},
                method="POST",
            )
        except urllib.error.HTTPError as e:
            _logger.warning("An error occurred: %s", e)

        group_home = self.env["website.page"].search(
            [("url", "=", f"/{self.test_group.website_slug}/home")]
        )
        self.assertFalse(
            group_home, "Non-member should not be able to create group homepage"
        )

    def test_04_user_lifecycle_unpublish(self):
        self.authenticate(self.user_a.login, self.user_a.login)

        page = self.env["website.page"].create(
            {
                "url": f"/{self.user_a.website_slug}/mypage",
                "name": "My Page",
                "website_published": True,
                "type": "qweb",
                "owner_user_id": self.user_a.id,
            }
        )

        blog = self.env["blog.blog"].search([], limit=1) or self.env[
            "blog.blog"
        ].create({"name": "B"})

        post = self.env["blog.post"].create(
            {
                "name": "My Post",
                "blog_id": blog.id,
                "is_published": True,
                "owner_user_id": self.user_a.id,
            }
        )
        self.assertTrue(page.website_published, "Page should be published initially")
        self.assertTrue(post.is_published, "Post should be published initially")

        self.logout()
        self.authenticate("admin", "admin")

        self.env.cr.commit()
        self.user_a.with_context(test_mode=True).active = False
        self.env.cr.commit()

        for _ in range(20):
            self.env.cr.commit()
            self.env.invalidate_all()
            if not page.website_published:
                time.sleep(0.5)  # audit-ignore-sleep
                break
            time.sleep(0.5)  # audit-ignore-sleep

        page.invalidate_recordset()
        post.invalidate_recordset()

        self.assertFalse(
            page.website_published, "Page should be unpublished when user is archived"
        )
        self.assertFalse(
            post.is_published, "Post should be unpublished when user is archived"
        )

    def test_05_community_directory_opt_in(self):
        self.assertFalse(self.user_a.privacy_show_in_directory)

        self.env.cr.commit()
        response = self.url_open("/community")
        self.assertNotIn(
            self.user_a.name, response.text, "User should NOT be visible by default"
        )

        self.user_a.write({"privacy_show_in_directory": True})

        self.env.cr.commit()
        response = self.url_open("/community")
        self.assertIn(
            f"/{self.user_a.website_slug}",
            response.text,
            "User link should be visible after opt-in",
        )

    def test_06_group_report_button_visibility(self):
        self.test_group.write({"member_ids": [(4, self.user_a.id)]})

        arch_string = f"""<t name="Home" t-name="user_websites.home_{self.test_group.website_slug}">
            <t t-call="user_websites.template_default_home">
                <div id="wrap" class="oe_structure oe_empty"/>
            </t>
        </t>"""

        self.env["website.page"].create(
            {
                "url": f"/{self.test_group.website_slug}/home",
                "name": "Home",
                "is_published": True,
                "website_published": True,
                "type": "qweb",
                "arch": arch_string,
                "user_websites_group_id": self.test_group.id,
            }
        )

        report_button_text = b"Report Violation"
        url_group = f"/{self.test_group.website_slug}/home"

        self.authenticate(self.user_a.login, self.user_a.login)
        self.env.cr.commit()
        response = self.url_open(url_group)
        self.assertNotIn(
            report_button_text,
            response.content,
            "Group Member should NOT see the Report button.",
        )

        self.logout()
        self.env.cr.commit()
        response = self.url_open(url_group)
        self.assertIn(
            report_button_text,
            response.content,
            "Public visitor SHOULD see the report button.",
        )

    def test_07_public_cannot_create_group_site(self):
        self.authenticate(None, None)
        create_url = f"/{self.test_group.website_slug}/create_site"

        try:
            self.env.cr.commit()
            self.url_open(
                create_url,
                data={"csrf_token": odoo.http.Request.csrf_token(self)},
                method="POST",
            )
        except urllib.error.HTTPError as e:
            _logger.warning("An error occurred: %s", e)

        group_home = self.env["website.page"].search(
            [("url", "=", f"/{self.test_group.website_slug}/home")]
        )
        self.assertFalse(
            group_home, "Public Guest should not be able to create group homepage"
        )

    def test_08_group_inverse_relationships(self):
        # [@ANCHOR: test_group_blog_post_creation]
        # Tests [@ANCHOR: UX_CREATE_BLOG_POST]
        page = self.env["website.page"].create(
            {
                "url": f"/{self.test_group.website_slug}/test-page",
                "is_published": True,
                "user_websites_group_id": self.test_group.id,
                "type": "qweb",
            }
        )

        blog = self.env["blog.blog"].search([], limit=1) or self.env[
            "blog.blog"
        ].create({"name": "B"})
        post = self.env["blog.post"].create(
            {
                "name": "Group Post",
                "blog_id": blog.id,
                "user_websites_group_id": self.test_group.id,
                "is_published": True,
            }
        )

        self.assertIn(
            page,
            self.test_group.website_page_ids,
            "Page should appear in group's website_page_ids",
        )
        self.assertIn(
            post,
            self.test_group.blog_post_ids,
            "Post should appear in group's blog_post_ids",
        )

    def test_09_reserved_slug_validation(self):
        """
        Verify that protected terms (e.g., 'community', 'blog') are rejected
        to avoid intercepting core Odoo routes.
        """
        with self.assertRaises(ValidationError):
            self.env["res.users"].create(
                {
                    "name": "Community",
                    "login": "community_tester",
                    "email": "community_tester@example.com",
                    "website_slug": "community",
                }
            )
            self.env.flush_all()

        with self.assertRaises(ValidationError):
            self.env["user.websites.group"].create(
                {
                    "name": "Blog Group",
                    "website_slug": "blog",
                }
            )
            self.env.flush_all()
