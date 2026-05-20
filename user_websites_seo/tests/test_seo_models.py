# -*- coding: utf-8 -*-
from odoo.addons.hams_test.common import HamsTransactionCase
from odoo.exceptions import AccessError

class TestSEOModels(HamsTransactionCase):

    @classmethod
    def setUpClass(cls):
        super().setUpClass()
        cls.user_admin = cls.env.ref('base.user_admin')

        cls.regular_user1 = cls.env['res.users'].create({
            'name': 'Regular User 1',
            'login': 'reg1',
            'group_ids': [(6, 0, [cls.env.ref('base.group_portal').id])]
        })

        cls.regular_user2 = cls.env['res.users'].create({
            'name': 'Regular User 2',
            'login': 'reg2',
            'group_ids': [(6, 0, [cls.env.ref('base.group_portal').id])]
        })

        cls.group = cls.env['user.websites.group'].create({
            'name': 'Test SEO Group',
            'website_slug': 'test-seo-group',
            'member_ids': [(6, 0, [cls.regular_user1.id])]
        })

    def test_self_writeable_fields(self):
        # Tests [@ANCHOR: res_users_self_writeable_fields]
        # [@ANCHOR: test_self_writeable_fields]
        # Verified by [@ANCHOR: test_self_writeable_fields]
        """Test that SEO fields are added to writeable fields for users."""
        fields = self.env['res.users'].SELF_WRITEABLE_FIELDS
        seo_fields = [
            "website_meta_title",
            "website_meta_description",
            "website_meta_keywords",
            "website_meta_og_img",
            "seo_name",
        ]
        for field in seo_fields:
            self.assertIn(field, fields, f"Field {field} should be writeable")

    def test_check_access_rule_res_users(self):
        # Tests [@ANCHOR: res_users_seo_write_elevation]
        # [@ANCHOR: test_check_access_rule_res_users]
        # Verified by [@ANCHOR: test_check_access_rule_res_users]
        """Test that a user can write to their own SEO fields but not others."""
        # reg1 can write to their own profile
        reg1_record = self.regular_user1.with_user(self.regular_user1)
        # Should not raise exception
        reg1_record.write({'website_meta_title': 'My Title'})

        # reg1 cannot write to reg2
        reg2_record_by_reg1 = self.regular_user2.with_user(self.regular_user1)
        with self.assertRaises(AccessError):
            reg2_record_by_reg1.write({'website_meta_title': 'Hacked Title'})
            self.env.flush_all()

    def test_check_access_rule_user_websites_group(self):
        # Tests [@ANCHOR: user_websites_group_seo_write_elevation]
        # [@ANCHOR: test_check_access_rule_user_websites_group]
        # Verified by [@ANCHOR: test_check_access_rule_user_websites_group]
        """Test that a user can write to a group they are a member of, but not others."""
        # reg1 is a member, can write
        group_by_reg1 = self.group.with_user(self.regular_user1)
        # Should not raise exception
        group_by_reg1.write({'website_meta_title': 'Group Title'})

        # reg2 is not a member, cannot write
        group_by_reg2 = self.group.with_user(self.regular_user2)
        with self.assertRaises(AccessError):
            group_by_reg2.write({'website_meta_title': 'Hacked Group Title'})
            self.env.flush_all()

    def test_soft_dependency_docs_installation(self):
        # Tests [@ANCHOR: soft_dependency_docs_installation]
        # [@ANCHOR: test_soft_dependency_docs_installation]
        # Verified by [@ANCHOR: test_soft_dependency_docs_installation]
        """Verify that documentation is installed correctly via zero_sudo bootstrap."""
        # This test checks if the documentation is present in the knowledge/manual library
        article_model = None
        if 'knowledge.article' in self.env:
            article_model = 'knowledge.article'
        elif 'manual.article' in self.env:
            article_model = 'manual.article'

        if not article_model:
            self.skipTest("No knowledge or manual article model found")

        # The article should have been created during module installation by zero_sudo hook
        doc = self.env[article_model].search([('name', '=', 'User Websites SEO Guide')], limit=1)
        # It might also be named "User Websites SEO: Optimization Guide" if I changed it in documentation.html
        # but the manifest says "User Websites SEO Guide"
        if not doc:
            doc = self.env[article_model].search([('name', '=', 'User Websites SEO: Optimization Guide')], limit=1)

        self.assertTrue(doc, "Documentation article should have been created")
        self.assertIn("Optimization Guide", doc.body or "", "Documentation body should contain expected text")

    def test_xpath_rendering_res_users(self):
        # [@ANCHOR: test_xpath_rendering_res_users]
        # Verified by [@ANCHOR: test_xpath_rendering_res_users]
        """Prove that the SEO notebook page correctly renders in res.users."""
        res = self.env["res.users"].get_view(
            view_id=self.env.ref("base.view_users_form").id,
            view_type="form"
        )
        self.assertIn(
            'name="user_websites_seo_settings"',
            res["arch"],
            "The SEO notebook page must exist in the compiled res.users arch.",
        )
        self.assertIn(
            'name="website_meta_og_img"',
            res["arch"],
            "The Social Media (Open Graph Image) field must exist in the compiled res.users arch.",
        )

    def test_xpath_rendering_user_websites_group(self):
        # [@ANCHOR: test_xpath_rendering_user_websites_group]
        # Verified by [@ANCHOR: test_xpath_rendering_user_websites_group]
        """Prove that the SEO notebook page correctly renders in user.websites.group."""
        res = self.env["user.websites.group"].get_view(
            view_id=self.env.ref("user_websites.view_user_websites_group_form").id,
            view_type="form"
        )
        self.assertIn(
            'name="group_seo_settings"',
            res["arch"],
            "The SEO notebook page must exist in the compiled user.websites.group arch.",
        )
        self.assertIn(
            'name="website_meta_og_img"',
            res["arch"],
            "The Social Media (Open Graph Image) field must exist in the compiled user.websites.group arch.",
        )
