# -*- coding: utf-8 -*-
# Part of Odoo. See LICENSE file for full copyright and licensing details.
#
# This file is part of hams_open, an open source module.
# License: AGPL-3.0

import logging
from odoo import SUPERUSER_ID  # burn-ignore-superuser-rejection-test: see usage sites below
from odoo.tests import tagged
from odoo.addons.zero_sudo.tests.common import HamsTransactionCase
from odoo.exceptions import UserError, ValidationError

_logger = logging.getLogger(__name__)

@tagged('post_install', '-at_install')
class TestEdgeRoutingMixin(HamsTransactionCase):

    @classmethod
    def setUpClass(cls):
        super().setUpClass()
        cls.env = cls.env(context=dict(cls.env.context, tracking_disable=True))
        cls.User = cls.env['res.users']

    def test_auto_generate_slug_on_create(self):
        # Tests [@ANCHOR: edge_routing:COMM_mixin_create]
        user = self.User.create({
            'name': 'Test User Mixin 1',
            'login': 'test_user_mixin_1@example.com',
        })
        self.assertEqual(user.website_slug, 'test-user-mixin-1')

    def test_auto_generate_slug_collision(self):
        # Tests [@ANCHOR: edge_routing:COMM_check_slug_collision]

        # Tests [@ANCHOR: edge_routing:COMM_mixin_generate_unique_slug]
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
        # Tests [@ANCHOR: edge_routing:COMM_mixin_write]
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
        # Tests [@ANCHOR: edge_routing:COMM_get_routing_models]
        models = self.env['edge.routing.mixin']._get_routing_models()
        self.assertIn('res.users', models)

    def test_unlink_notifies_cache_invalidation_for_the_freed_slug(self):
        # Tests [@ANCHOR: edge_routing:COMM_mixin_unlink]
        # unlink() had zero direct test coverage -- confirm the record is
        # actually gone and its slug becomes resolvable again by a new
        # record (proving the old slug's cache entry was really cleared,
        # not just that unlink() didn't raise).
        user = self.User.create({
            'name': 'Unlink Slug User',
            'login': 'unlink_slug_user@example.com',
        })
        freed_slug = user.website_slug
        self.assertTrue(freed_slug)
        user_id = user.id
        user.unlink()
        self.assertFalse(self.User.browse(user_id).exists())

        user2 = self.User.create({
            'name': 'Unlink Slug User',
            'login': 'unlink_slug_user_2@example.com',
        })
        self.assertEqual(
            user2.website_slug,
            freed_slug,
            "The freed slug should be available for reuse once the owning "
            "record is gone and its cache entry invalidated.",
        )

    def test_get_record_by_slug_cache_removal(self):
        # res.users' get_record_by_slug() must not be decorated with
        # @distributed_cache(): its login-fallback branch resolves against
        # res.users.login, and routing_mixin.write()'s own cache-invalidation
        # hook only fires on website_slug/name changes, so a cached result
        # keyed on a slug that later matches a changed/new login would go
        # stale for the full 24h Redis TTL.
        #
        # Bug found while touching this file for the override_svc_uid fix
        # (2026-09-12): this test's original assertion (a bare
        # `method.clear_cache` lookup) could never fail either way --
        # distributed_cache()'s wrapper never sets a `clear_cache` attribute
        # at all (confirmed directly against
        # distributed_redis_cache/redis_cache.py), so the AttributeError
        # fired unconditionally regardless of decoration, and the test
        # passed the whole time res.users.get_record_by_slug WAS decorated.
        # `functools.wraps` (which distributed_cache() does use) sets
        # `__wrapped__` on the wrapper, which is what actually distinguishes
        # a decorated method from a plain one.
        method = self.User.__class__.get_record_by_slug
        self.assertFalse(
            hasattr(method, "__wrapped__"),  # burn-ignore-introspection
            "get_record_by_slug on res.users should not be wrapped by "
            "@distributed_cache() -- its login-fallback branch isn't "
            "covered by the mixin's write()-based cache invalidation.",
        )

    def test_get_record_by_slug_no_longer_accepts_a_caller_supplied_service_uid(self):
        """
        Bug-hunt fix (docs/bug_hunt_claims/.../override_svc_uid, 2026-09-12):
        get_record_by_slug() (and the since-deleted get_record_by_domain()/
        get_target_slug_by_domain()) used to accept a caller-supplied
        `override_svc_uid` and run self.with_user(override_svc_uid).env
        with zero validation -- any authenticated RPC caller (this method
        is public, no leading underscore) could pick an arbitrary uid to
        search under, bypassing whatever ir.rule scoping would normally
        apply to them. No real caller anywhere in the codebase ever passed
        this parameter, so it was removed entirely rather than validated.
        Prove the parameter is genuinely gone, not just unused -- a
        TypeError here is exactly what protects against the RPC path,
        since Odoo's own dispatch passes kwargs straight through to the
        method signature.
        """
        with self.assertRaises(TypeError):
            self.env["user.websites.group"].get_record_by_slug(
                "some-slug", override_svc_uid=SUPERUSER_ID  # burn-ignore-superuser-rejection-test
            )

    def test_get_target_slug_by_domain_no_longer_accepts_a_caller_supplied_service_uid(self):
        # Tests [@ANCHOR: edge_routing:COMM_domain_get_target_slug_by_domain]
        with self.assertRaises(TypeError):
            self.env["edge.routing.domain"].get_target_slug_by_domain(
                "example.com", override_svc_uid=SUPERUSER_ID  # burn-ignore-superuser-rejection-test
            )

    def test_edge_routing_service_account_sql_check(self):
        # [@ANCHOR: test_edge_routing_service_account_sql_check]
        """
        Verify that the raw SQL check for the service account safely executes.
        """
        self.env.cr.execute("SELECT 1 FROM ir_model_data WHERE module=%s AND name=%s", ('edge_routing', 'edge_routing_service_account'))
        self.env.cr.fetchall() # Should not raise

    def test_directly_setting_a_reserved_slug_is_rejected_on_write(self):
        # Tests [@ANCHOR: edge_routing:COMM_check_reserved_slugs]
        # _check_reserved_slugs had zero direct test coverage -- confirm
        # a user can't hand-set website_slug to a reserved route name
        # (auto-generation already avoids these via RESERVED_SLUGS
        # seeding _generate_unique_slug's existing_slugs set, but nothing
        # was proving the constraint itself actually fires on a direct
        # write that bypasses generation).
        user = self.User.create({
            'name': 'Test User Reserved Slug',
            'login': 'test_user_reserved_slug@example.com',
        })
        with self.assertRaises(ValidationError):
            user.write({'website_slug': 'shack'})
            self.env.flush_all()

    def test_directly_setting_a_reserved_slug_is_rejected_on_create(self):
        with self.assertRaises(ValidationError):
            self.User.create({
                'name': 'Test User Reserved Slug 2',
                'login': 'test_user_reserved_slug_2@example.com',
                'website_slug': 'arrl',
            })
            self.env.flush_all()
