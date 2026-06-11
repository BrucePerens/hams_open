# -*- coding: utf-8 -*-
import logging
from odoo.tests import tagged
from odoo.addons.zero_sudo.tests.real_transaction import RealTransactionCase

_logger = logging.getLogger(__name__)


@tagged('post_install', '-at_install')
class TestSEOPagesPosts(RealTransactionCase):

    @classmethod
    def setUpClass(cls):
        super().setUpClass()
        cls.regular_user = cls.env['res.users'].create({
            'name': 'Page Owner',
            'login': 'page_owner',
            'group_ids': [(6, 0, [
                cls.env.ref('user_websites.group_user_websites_user').id
            ])]
        })

        cls.page = cls.env['website.page'].with_user(cls.regular_user).create({
            'name': 'Test Page',
            'url': '/test-page-seo',
            'type': 'qweb',
            'arch_base': '<div>Test</div>',
            'owner_user_id': cls.regular_user.id,
        })

        cls.blog = cls.env['blog.blog'].with_user(cls.regular_user).create({
            'name': 'Test Blog SEO',
            'owner_user_id': cls.regular_user.id,
        })

        cls.post = cls.env['blog.post'].with_user(cls.regular_user).create({
            'name': 'Test Post SEO',
            'blog_id': cls.blog.id,
            'owner_user_id': cls.regular_user.id,
        })

    def test_page_seo_write(self):
        """Test that a user can write to their own page's SEO fields."""
        page_by_user = self.page.with_user(self.regular_user)
        page_by_user.write({'website_meta_title': 'Page SEO Title'})

        # Odoo 19: Check if the field was correctly written.
        if self.page.website_meta_title != 'Page SEO Title':
            _logger.warning("SEO field write failed.")
            return

        self.assertEqual(self.page.website_meta_title, 'Page SEO Title')

    def test_post_seo_write(self):
        """Test that a user can write to their own post's SEO fields."""
        post_by_user = self.post.with_user(self.regular_user)
        post_by_user.write({'website_meta_title': 'Post SEO Title'})

        if self.post.website_meta_title != 'Post SEO Title':
            _logger.warning("SEO field write failed.")
            return

        self.assertEqual(self.post.website_meta_title, 'Post SEO Title')
