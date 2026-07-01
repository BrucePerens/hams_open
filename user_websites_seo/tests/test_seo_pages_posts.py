# -*- coding: utf-8 -*-
from odoo.tests import tagged
from odoo.addons.zero_sudo.tests.real_transaction import RealTransactionCase


@tagged("post_install", "-at_install")
class TestSEOPagesPosts(RealTransactionCase):

    @classmethod
    def setUpClass(cls):
        super().setUpClass()
        cls.regular_user = cls.env["res.users"].create(
            {
                "name": "Page Owner",
                "login": "page_owner",
                "group_ids": [
                    (6, 0, [cls.env.ref("user_websites.group_user_websites_user").id])
                ],
            }
        )

        cls.page = (
            cls.env["website.page"]
            .with_user(cls.regular_user)
            .create(
                {
                    "name": "Test Page",
                    "url": "/test-page-seo",
                    "type": "qweb",
                    "arch_base": "<div>Test</div>",
                    "owner_user_id": cls.regular_user.id,
                }
            )
        )

        cls.blog = (
            cls.env["blog.blog"]
            .with_user(cls.regular_user)
            .create(
                {
                    "name": "Test Blog SEO",
                    "owner_user_id": cls.regular_user.id,
                }
            )
        )

        cls.post = (
            cls.env["blog.post"]
            .with_user(cls.regular_user)
            .create(
                {
                    "name": "Test Post SEO",
                    "blog_id": cls.blog.id,
                    "owner_user_id": cls.regular_user.id,
                }
            )
        )

    def test_page_seo_write(self):
        """Test that a user can write to their own page's SEO fields."""
        page_by_user = self.page.with_user(self.regular_user)
        page_by_user.write({"website_meta_title": "Page SEO Title"})
        self.page.invalidate_recordset()
        self.assertEqual(
            self.page.website_meta_title,
            "Page SEO Title",
        )

    def test_post_seo_write(self):
        """Test that a user can write to their own post's SEO fields."""
        post_by_user = self.post.with_user(self.regular_user)
        post_by_user.write({"website_meta_title": "Post SEO Title"})
        self.post.invalidate_recordset()
        self.assertEqual(
            self.post.website_meta_title,
            "Post SEO Title",
        )

    def test_blog_seo_write(self):
        """Test that a user can write to their own blog's SEO fields."""
        blog_by_user = self.blog.with_user(self.regular_user)
        blog_by_user.write({"website_meta_title": "Blog SEO Title"})
        self.blog.invalidate_recordset()
        self.assertEqual(
            self.blog.website_meta_title,
            "Blog SEO Title",
        )

    def test_soft_dependency_docs_installation(self):
        # [@ANCHOR: test_soft_dependency_docs_installation]
        utils = self.env["zero_sudo.security.utils"]
        val = utils._get_system_param("user_websites_seo.docs_installed")
        self.assertEqual(val, "True")
