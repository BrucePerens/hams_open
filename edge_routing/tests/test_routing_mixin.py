# -*- coding: utf-8 -*-
from odoo.tests import tagged
from odoo.addons.zero_sudo.tests.common import HamsTransactionCase
from odoo.exceptions import UserError

@tagged('post_install', '-at_install')
class TestEdgeRoutingMixin(HamsTransactionCase):

    @classmethod
    def setUpClass(cls):
        super().setUpClass()
        cls.env = cls.env(context=dict(cls.env.context, tracking_disable=True))
        cls.User = cls.env['res.users']

    def test_auto_generate_slug_on_create(self):
        user = self.User.create({
            'name': 'Test User Mixin 1',
            'login': 'test_user_mixin_1@example.com',
        })
        self.assertEqual(user.website_slug, 'test-user-mixin-1')

    def test_auto_generate_slug_collision(self):
        user1 = self.User.create({
            'name': 'Test User Mixin 2',
            'login': 'test_user_mixin_2@example.com',
        })
        self.assertEqual(user1.website_slug, 'test-user-mixin-2')
        
        user2 = self.User.create({
            'name': 'Test User Mixin 2',
            'login': 'test_user_mixin_2_alt@example.com',
        })
        self.assertEqual(user2.website_slug, 'test-user-mixin-2-1')

    def test_batch_write_crash_prevention(self):
        user1 = self.User.create({'name': 'U1', 'login': 'u1@ex.com'})
        user2 = self.User.create({'name': 'U2', 'login': 'u2@ex.com'})
        
        users = user1 | user2
        with self.assertRaises(UserError):
            users.write({'website_slug': 'shared-slug'})
            self.env.flush_all()

    def test_batch_write_auto_slug(self):
        user1 = self.User.create({'name': 'NoSlug1', 'login': 'ns1@ex.com'})
        user2 = self.User.create({'name': 'NoSlug2', 'login': 'ns2@ex.com'})
        
        user1.website_slug = False
        user2.website_slug = False
        
        users = user1 | user2
        users.write({'name': 'NewName'})
        
        self.assertEqual(user1.website_slug, 'newname')
        self.assertEqual(user2.website_slug, 'newname-1')

    def test_write_clears_slug(self):
        user = self.User.create({
            'name': 'Clear Slug User',
            'login': 'clear_slug@ex.com',
        })
        self.assertTrue(user.website_slug)
        user.write({'website_slug': False})
        self.assertFalse(user.website_slug)
