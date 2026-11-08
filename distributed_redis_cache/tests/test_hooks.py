# SPDX-License-Identifier: AGPL-3.0-or-later

# -*- coding: utf-8 -*-
from odoo.addons.zero_sudo.tests.common import HamsTransactionCase
from odoo.tests import tagged

from odoo.addons.distributed_redis_cache.hooks import (
    CACHE_MANAGER_KEY_NAME,
    KEY_MANAGER_XMLID,
    unregister_cache_manager_key,
)

LEGACY_USER_XMLID = "distributed_redis_cache.cache_manager_service_internal"
LEGACY_KEY_FILE = "/opt/hams/etc/keys/cache_manager.env"


@tagged('post_install', '-at_install')
class TestDistributedRedisCacheHooks(HamsTransactionCase):
    """Versions before 1.1 registered the "Redis Cache Manager" key at install and a database
    installed then still has the row and its API key. The cache-manager daemon never used that key,
    so unregister_cache_manager_key() (run by migrations/1.1) removes both."""

    def _register_as_version_1_0_did(self):
        # Exactly the call the removed post_init_hook made.
        self.env["daemon.key.registry"].register_daemon(
            daemon_name=CACHE_MANAGER_KEY_NAME,
            user_xml_id=LEGACY_USER_XMLID,
            env_file_path=LEGACY_KEY_FILE,
        )

    def _registries(self):
        manager = self.env.ref(KEY_MANAGER_XMLID)
        return self.env["daemon.key.registry"].with_user(manager).search(
            [("name", "=", CACHE_MANAGER_KEY_NAME)]
        )

    def _api_keys(self, user):
        manager = self.env.ref(KEY_MANAGER_XMLID)
        return self.env["res.users.apikeys"].with_user(manager).search(
            [("user_id", "=", user.id), ("name", "=", f"{CACHE_MANAGER_KEY_NAME}_key")]
        )

    def test_unregister_removes_the_row_and_revokes_its_key(self):
        # [@ANCHOR: distributed_redis_cache:test_unregister_cache_manager_key]
        # Tests [@ANCHOR: distributed_redis_cache:unregister_cache_manager_key]
        user = self.env.ref(LEGACY_USER_XMLID)
        self._register_as_version_1_0_did()
        self.assertEqual(len(self._registries()), 1)
        self.assertEqual(len(self._api_keys(user)), 1)

        removed = unregister_cache_manager_key(self.env)

        self.assertEqual(removed, 1)
        self.assertFalse(self._registries())
        self.assertFalse(self._api_keys(user))
        self.assertTrue(user.exists(), "the service account itself stays")
        self.assertTrue(user.active)

    def test_unregister_is_idempotent(self):
        # Tests [@ANCHOR: distributed_redis_cache:unregister_cache_manager_key]
        self._register_as_version_1_0_did()
        self.assertEqual(unregister_cache_manager_key(self.env), 1)
        self.assertEqual(unregister_cache_manager_key(self.env), 0)
        self.assertFalse(self._registries())

    def test_module_no_longer_registers_the_key_at_install(self):
        # Tests [@ANCHOR: distributed_redis_cache:unregister_cache_manager_key]
        # A fresh install has no row: the module declares no post_init_hook (the registry is only
        # filled by a database installed before 1.1, which the migration cleans).
        manifest = self.env["ir.module.module"].get_module_info("distributed_redis_cache")
        self.assertFalse(manifest.get("post_init_hook"))
