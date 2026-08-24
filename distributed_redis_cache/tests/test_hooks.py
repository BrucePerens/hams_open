# This software is distributed under the terms of the Affero General Public License (AGPL-3).
# SPDX-License-Identifier: AGPL-3.0-or-later

# -*- coding: utf-8 -*-
import os

from odoo.addons.zero_sudo.tests.common import HamsTransactionCase
from odoo.tests import tagged

from odoo.addons.distributed_redis_cache.hooks import post_init_hook


@tagged('post_install', '-at_install')
class TestDistributedRedisCacheHooks(HamsTransactionCase):
    """post_init_hook() is the real, currently-shipped template
    hams_s3/hooks.py's own post_init_hook explicitly mirrors -- this had
    the same untested-since-install gap as that one, unaddressed until
    now: register_daemon() being called with the right daemon_name/
    user_xml_id/env_file_path was never actually exercised, only assumed
    correct because the pattern "looked like" the (also-untested, until
    hams_s3.tests.test_hooks) copy of it."""

    def test_post_init_hook_registers_the_cache_manager_daemon(self):
        self.env.ref("distributed_redis_cache.cache_manager_service_internal")

        post_init_hook(self.env)

        registry = self.env["daemon.key.registry"].search(
            [("name", "=", "Redis Cache Manager")], limit=1
        )
        self.assertTrue(registry, "post_init_hook did not register the Redis Cache Manager daemon")
        self.assertEqual(registry.env_file_path, "/opt/hams/etc/keys/cache_manager.env")
        self.assertTrue(os.path.exists(registry.env_file_path))

    def test_post_init_hook_is_idempotent_on_reinstall(self):
        post_init_hook(self.env)
        post_init_hook(self.env)

        registries = self.env["daemon.key.registry"].search(
            [("name", "=", "Redis Cache Manager")]
        )
        self.assertEqual(len(registries), 1)
