# SPDX-License-Identifier: AGPL-3.0-or-later

# -*- coding: utf-8 -*-
import importlib.util
import os
from odoo.addons.zero_sudo.tests.common import HamsTransactionCase
from odoo.tests import tagged

from odoo.addons.distributed_redis_cache.hooks import (
    CACHE_MANAGER_KEY_NAME,
    KEY_MANAGER_XMLID,
    unregister_cache_manager_key,
)

MIGRATION_FILE = os.path.join(
    os.path.dirname(os.path.dirname(os.path.abspath(__file__))), "migrations", "1.1", "post-unregister-cache-manager-key.py"
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

    def _api_key_count(self, user):
        self.env.cr.execute(
            "SELECT count(*) FROM res_users_apikeys WHERE user_id = %s AND name = %s",
            (user.id, f"{CACHE_MANAGER_KEY_NAME}_key"),
        )
        return self.env.cr.fetchone()[0]

    def test_unregister_removes_the_row_and_revokes_its_key(self):
        # [@ANCHOR: distributed_redis_cache:test_unregister_cache_manager_key]
        # Tests [@ANCHOR: distributed_redis_cache:unregister_cache_manager_key]
        user = self.env.ref(LEGACY_USER_XMLID)
        self._register_as_version_1_0_did()
        self.assertEqual(len(self._registries()), 1)
        self.assertEqual(self._api_key_count(user), 1)

        removed = unregister_cache_manager_key(self.env)

        self.assertEqual(removed, 1)
        self.assertFalse(self._registries())
        self.assertEqual(self._api_key_count(user), 0)
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

    def _migration(self):
        spec = importlib.util.spec_from_file_location("distributed_redis_cache_migration_1_1", MIGRATION_FILE)
        module = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(module)
        return module

    def test_migration_removes_the_registration_of_an_installed_database_only(self):
        # Tests [@ANCHOR: distributed_redis_cache:migrate]
        user = self.env.ref(LEGACY_USER_XMLID)
        self._register_as_version_1_0_did()
        migration = self._migration()

        migration.migrate(self.env.cr, None)  # a fresh install has no version: nothing is removed
        self.assertEqual(len(self._registries()), 1)
        self.assertEqual(self._api_key_count(user), 1)

        migration.migrate(self.env.cr, "1.0")  # an installed database drops the row and the key
        self.assertFalse(self._registries())
        self.assertEqual(self._api_key_count(user), 0)

        migration.migrate(self.env.cr, "1.0")  # and the migration may run again
        self.assertFalse(self._registries())
