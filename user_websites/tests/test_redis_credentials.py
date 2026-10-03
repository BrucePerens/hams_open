# -*- coding: utf-8 -*-
# Copyright © Bruce Perens K6BP.
# SPDX-License-Identifier: AGPL-3.0-or-later
from odoo.tests import tagged
from odoo.addons.zero_sudo.tests.common import HamsTransactionCase
from odoo.addons.distributed_redis_cache.redis_pool import REDIS_PASS_DEFAULT, REDIS_USERNAME_DEFAULT
from odoo.addons.user_websites.controllers import main as controllers_main
from odoo.addons.user_websites.models import website_page


@tagged("post_install", "-at_install")
class TestUserWebsitesRedisCredentials(HamsTransactionCase):
    """user_websites keeps two Redis pools of its own (page-view counters). Production Redis refuses
    unauthenticated clients since 2026-10-03, so both must carry the redis.env credentials that
    distributed_redis_cache's shared pool uses, and must reach the real Redis with them."""

    def test_both_page_view_pools_authenticate_like_the_shared_pool(self):
        for module in (website_page, controllers_main):
            with self.subTest(module=module.__name__):
                kwargs = module.redis_pool.connection_kwargs
                self.assertEqual(kwargs.get("username"), REDIS_USERNAME_DEFAULT)
                self.assertEqual(kwargs.get("password"), REDIS_PASS_DEFAULT)
                self.assertTrue(module.redis_client.ping())
