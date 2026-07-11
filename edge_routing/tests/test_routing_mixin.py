# -*- coding: utf-8 -*-
# Part of Odoo. See LICENSE file for full copyright and licensing details.
#
# This file is part of hams_open, an open source module.
# License: AGPL-3.0

import logging
from odoo.tests import tagged
from odoo.addons.zero_sudo.tests.common import HamsTransactionCase
from odoo.exceptions import UserError

_logger = logging.getLogger(__name__)

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

    def test_empty_slug_unique_violation(self):
        user1 = self.User.create({'name': 'U1', 'login': 'u1_empty@ex.com'})
        user2 = self.User.create({'name': 'U2', 'login': 'u2_empty@ex.com'})
        # Should not raise UniqueViolation when both are empty string
        user1.write({'website_slug': ''})
        user2.write({'website_slug': ''})
        self.env.flush_all()
        self.assertFalse(user1.website_slug)
        self.assertFalse(user2.website_slug)

    def test_get_routing_models_dynamic(self):
        models = self.env['edge.routing.mixin']._get_routing_models()
        self.assertIn('res.users', models)

    def test_get_record_by_slug_cache_removal(self):
        # res.users get_record_by_slug should not be decorated with @distributed_cache
        # If it is, the class method will have the 'clear_cache' attribute from the decorator
        method = self.User.__class__.get_record_by_slug
        has_clear_cache = getattr(method, 'clear_cache', None) is not None
        self.assertFalse(has_clear_cache, "get_record_by_slug on res.users should not have @distributed_cache")
