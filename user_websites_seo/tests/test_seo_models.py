# This software is distributed under the terms of the Affero General Public License (AGPL-3).

# -*- coding: utf-8 -*-
from odoo.addons.zero_sudo.tests.real_transaction import RealTransactionCase
from odoo.exceptions import AccessError


from odoo.tests import tagged


@tagged("post_install", "-at_install")
class TestSEOModels(RealTransactionCase):

    @classmethod
    def setUpClass(cls):
        super().setUpClass()
        cls.user_admin = cls.env.ref("base.user_admin")

        cls.regular_user1 = cls.env["res.users"].create(
            {
                "name": "Regular User 1",
                "login": "reg1",
                "group_ids": [(6, 0, [cls.env.ref("base.group_portal").id])],
            }
        )

        cls.regular_user2 = cls.env["res.users"].create(
            {
                "name": "Regular User 2",
                "login": "reg2",
                "group_ids": [(6, 0, [cls.env.ref("base.group_portal").id])],
            }
        )

        cls.group = cls.env["user.websites.group"].create(
            {
                "name": "Test SEO Group",
                "website_slug": "test-seo-group",
                "member_ids": [(6, 0, [cls.regular_user1.id])],
            }
        )

    def test_self_writeable_fields(self):
        # Tests [@ANCHOR: COMM_res_users_self_writeable_fields]

        # [@ANCHOR: COMM_test_self_writeable_fields]
        """Test that SEO fields are added to writeable fields for users."""
        fields = self.env["res.users"].SELF_WRITEABLE_FIELDS
        seo_fields = [
            "website_meta_title",
            "website_meta_description",
            "website_meta_keywords",
            "website_meta_og_img",
            "seo_name",
        ]
        for f in seo_fields:
            self.assertIn(f, fields)

    def test_check_access_rule_res_users(self):
        # Tests [@ANCHOR: COMM_res_users_seo_write_elevation]

        # [@ANCHOR: COMM_test_check_access_rule_res_users]
        """Test that a user can write to their own SEO fields but not others."""
        # reg1 can write to their own profile
        reg1_record = self.regular_user1.with_user(self.regular_user1)
        # Should not raise exception
        reg1_record.write({"website_meta_title": "My Title"})

        # reg1 cannot write to reg2
        reg2_record_by_reg1 = self.regular_user2.with_user(self.regular_user1)
        msg = "A user MUST NOT be able to modify the SEO of another user."
        with self.assertRaises(AccessError, msg=msg):
            reg2_record_by_reg1.write({"website_meta_title": "Hacked Title"})
            self.env.flush_all()

    def test_check_access_rule_user_websites_group(self):
        # Tests [@ANCHOR: COMM_user_websites_group_seo_write_elevation]

        # [@ANCHOR: COMM_test_check_access_rule_user_websites_group]
        """Test that a user can write to a group they are a member of."""
        # reg1 is a member, can write
        group_by_reg1 = self.group.with_user(self.regular_user1)
        # Should not raise exception
        group_by_reg1.write({"website_meta_title": "Group Title"})

        # reg2 is not a member, cannot write
        group_by_reg2 = self.group.with_user(self.regular_user2)
        msg = "A user MUST NOT be able to modify SEO of not owned group."
        with self.assertRaises(AccessError, msg=msg):
            group_by_reg2.write({"website_meta_title": "Hacked Group Title"})
            self.env.flush_all()

    def test_xpath_rendering_res_users(self):
        # [@ANCHOR: COMM_test_xpath_rendering_res_users]
        """Prove that the SEO notebook page correctly renders in res.users."""
        res = self.env["res.users"].get_view(
            view_id=self.env.ref("base.view_users_form").id, view_type="form"
        )
        self.assertIn(
            'name="user_websites_seo_settings"',
            res["arch"],
            "The SEO notebook page must exist in res.users arch.",
        )
        self.assertIn(
            'name="website_meta_og_img"',
            res["arch"],
            "The Social Media field must exist in res.users arch.",
        )

    def test_xpath_rendering_user_websites_group(self):
        # [@ANCHOR: COMM_test_xpath_rendering_user_websites_group]
        """Prove that the SEO notebook page renders in user.websites.group."""
        res = self.env["user.websites.group"].get_view(
            view_id=self.env.ref("user_websites.view_user_websites_group_form").id,
            view_type="form",
        )
        self.assertIn(
            'name="group_seo_settings"',
            res["arch"],
            "The SEO notebook page must exist in user.websites.group arch.",
        )
        self.assertIn(
            'name="website_meta_og_img"',
            res["arch"],
            "The Social Media field must exist in user.websites.group arch.",
        )

    def test_xpath_rendering_pages_posts(self):
        # [@ANCHOR: COMM_test_xpath_rendering_pages_posts]
        """Prove that the SEO page renders in website.page and blog.post."""
        # Test website.page
        res_page = self.env["website.page"].get_view(
            view_id=self.env.ref("website.website_pages_form_view").id, view_type="form"
        )
        arch = res_page["arch"]
        msg = "The SEO page must exist in website.page arch."
        self.assertIn('name="seo_settings"', arch, msg)

        # Test blog.post
        res_post = self.env["blog.post"].get_view(
            view_id=self.env.ref("website_blog.view_blog_post_form").id,
            view_type="form",
        )
        arch = res_post["arch"]
        msg = "The SEO page must exist in blog.post arch."
        self.assertIn('name="seo_settings"', arch, msg)
