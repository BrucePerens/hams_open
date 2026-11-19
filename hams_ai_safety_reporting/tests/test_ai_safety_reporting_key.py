# Copyright © Bruce Perens K6BP.
# SPDX-License-Identifier: AGPL-3.0-or-later
"""The AI safety reporting service account gets a daemon key through the Daemon Key Manager.

The Simulated Band bots and the Official Observer file a safety ticket with a key kept in
``hooks.KEY_FILE``. These tests run the registration for real (the registry row, the API key and the
key file are all real) against scratch file names, so a developer box's own key file is never overwritten. The family
group is a real one the provisioning created on every test box (``hamsd_ncvec_sync``, as daemon_key_manager's own tests use):
the key manager writes a group's key file only inside that group's key directory.
"""
import grp
import importlib.util
import os
import stat

from odoo.tests import tagged

from odoo.addons.hams_ai_safety_reporting import hooks
from odoo.addons.zero_sudo.tests.real_transaction import RealTransactionCase

SCRATCH_GROUP = "hamsd_ncvec_sync"
SCRATCH_KEY_FILE = "/opt/hams/etc/keys/ncvec_sync/test_ai_safety_reporting_key.env"
SCRATCH_OLD_KEY_FILE = "/opt/hams/etc/keys/test_ai_safety_reporting_key_old.env"
MODULE_DIR = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
MIGRATION_FILE = os.path.join(MODULE_DIR, "migrations", "1.1", "post-register-ai-safety-reporting-key.py")
MOVE_MIGRATION_FILE = os.path.join(MODULE_DIR, "migrations", "1.2", "post-move-ai-safety-reporting-key.py")


@tagged("post_install", "-at_install", "security")
class TestAiSafetyReportingKey(RealTransactionCase):
    def setUp(self):
        super().setUp()
        self.safe_patch_object(hooks, "KEY_FILE", new=SCRATCH_KEY_FILE)
        self.safe_patch_object(hooks, "OLD_KEY_FILE", new=SCRATCH_OLD_KEY_FILE)
        self.safe_patch_object(hooks, "OS_GROUP", new=SCRATCH_GROUP)
        self.addCleanup(self._remove_scratch_key_file)
        self._remove_scratch_key_file()
        self.addCleanup(lambda: self._registries().unlink())

    def _remove_scratch_key_file(self):
        for path in (SCRATCH_KEY_FILE, SCRATCH_OLD_KEY_FILE):
            if os.path.lexists(path):
                os.remove(path)

    def _registries(self):
        manager = self.env.ref("daemon_key_manager.user_daemon_key_manager_service")
        return self.env["daemon.key.registry"].with_user(manager).search(
            [("env_file_path", "in", (SCRATCH_KEY_FILE, SCRATCH_OLD_KEY_FILE))]
        )

    def test_registering_issues_the_key_for_the_create_only_account_and_is_idempotent(self):
        # [@ANCHOR: hams_ai_safety_reporting:test_register_ai_safety_reporting_key]
        # Tests [@ANCHOR: hams_ai_safety_reporting:register_ai_safety_reporting_key]
        # Tests [@ANCHOR: hams_ai_safety_reporting:post_init_hook]
        service_uid = self.env["zero_sudo.security.utils"]._get_service_uid(hooks.SERVICE_XMLID)

        hooks.post_init_hook(self.env)
        registry = self._registries()
        self.assertEqual(len(registry), 1, "one registry row for the safety reporting key")
        self.assertEqual(registry.name, hooks.KEY_NAME)
        self.assertEqual(registry.user_id.id, service_uid)
        self.assertEqual(registry.os_group, SCRATCH_GROUP, "the units run as the family account: group-readable")
        self.assertEqual(registry.env_file_path, SCRATCH_KEY_FILE)
        self.assertEqual(stat.S_IMODE(os.stat(SCRATCH_KEY_FILE).st_mode), 0o640)
        self.assertEqual(os.stat(SCRATCH_KEY_FILE).st_gid, grp.getgrnam(SCRATCH_GROUP).gr_gid)
        with open(SCRATCH_KEY_FILE, encoding="utf-8") as key_file:
            content = key_file.read()
        self.assertIn("ODOO_RPC_LOGIN=" + registry.user_id.login, content)
        self.assertIn("ODOO_RPC_KEY=", content)

        # Registering again (a re-install, the migration on an installed database) updates the same row.
        hooks.register_ai_safety_reporting_key(self.env)
        self.assertEqual(self._registries(), registry)

    def test_a_row_registered_by_hand_under_another_name_keeps_its_name(self):
        # Tests [@ANCHOR: hams_ai_safety_reporting:register_ai_safety_reporting_key]
        # hams.com's row was added by hand: if its name differed from KEY_NAME, a second row for the
        # same (unique) file path would be refused and the upgrade would stop.
        manager = self.env.ref("daemon_key_manager.user_daemon_key_manager_service")
        self.env["daemon.key.registry"].with_user(manager).create(
            {
                "name": "Hand Made Safety Row",
                "user_id": self.env["zero_sudo.security.utils"]._get_service_uid(hooks.SERVICE_XMLID),
                "env_file_path": SCRATCH_OLD_KEY_FILE,
            }
        )

        hooks.register_ai_safety_reporting_key(self.env)

        registry = self._registries()
        self.assertEqual(len(registry), 1)
        self.assertEqual(registry.name, "Hand Made Safety Row")
        self.assertEqual((registry.env_file_path, registry.os_group), (SCRATCH_KEY_FILE, SCRATCH_GROUP), "moved to the family directory")
        self.assertTrue(os.path.exists(SCRATCH_KEY_FILE), "the key was minted and written")

    def test_a_host_without_the_family_group_registers_and_moves_nothing(self):
        # Tests [@ANCHOR: hams_ai_safety_reporting:register_ai_safety_reporting_key]
        # The key manager refuses to write a group's key file before provisioning created the account, and an upgrade that
        # revoked the working key without writing its replacement would leave the bots with none.
        self.safe_patch_object(hooks, "OS_GROUP", new="hamsd_no_such_family_zz")
        manager = self.env.ref("daemon_key_manager.user_daemon_key_manager_service")
        self.env["daemon.key.registry"].with_user(manager).create(
            {
                "name": "Hand Made Safety Row",
                "user_id": self.env["zero_sudo.security.utils"]._get_service_uid(hooks.SERVICE_XMLID),
                "env_file_path": SCRATCH_OLD_KEY_FILE,
            }
        )
        self.assertFalse(hooks.register_ai_safety_reporting_key(self.env))
        registry = self._registries()
        self.assertEqual((registry.env_file_path, registry.os_group), (SCRATCH_OLD_KEY_FILE, False), "the row stays where it was")
        self.assertFalse(os.path.exists(SCRATCH_KEY_FILE))

    def test_the_migration_registers_the_key_on_an_installed_database_only(self):
        # Tests [@ANCHOR: hams_ai_safety_reporting:migrate]
        spec = importlib.util.spec_from_file_location("hams_ai_safety_reporting_migration_1_1", MIGRATION_FILE)
        migration = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(migration)

        migration.migrate(self.env.cr, None)  # a fresh install has no version: post_init_hook does the work
        self.assertFalse(self._registries())

        migration.migrate(self.env.cr, "1.0")  # an installed database registers the key now
        registry = self._registries()
        self.assertEqual(len(registry), 1)
        self.assertTrue(os.path.exists(SCRATCH_KEY_FILE), "the key was minted and written")

        migration.migrate(self.env.cr, "1.0")  # running it again updates the same row
        self.assertEqual(self._registries(), registry)

    def test_the_1_2_migration_moves_the_row_of_an_installed_1_1_database_and_skips_older_ones(self):
        # Tests [@ANCHOR: hams_ai_safety_reporting:migrate_move_key]
        spec = importlib.util.spec_from_file_location("hams_ai_safety_reporting_migration_1_2", MOVE_MIGRATION_FILE)
        migration = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(migration)
        manager = self.env.ref("daemon_key_manager.user_daemon_key_manager_service")
        self.env["daemon.key.registry"].with_user(manager).create(
            {
                "name": "Hand Made Safety Row",
                "user_id": self.env["zero_sudo.security.utils"]._get_service_uid(hooks.SERVICE_XMLID),
                "env_file_path": SCRATCH_OLD_KEY_FILE,
            }
        )

        migration.migrate(self.env.cr, None)  # a fresh install: post_init_hook does the work
        migration.migrate(self.env.cr, "1.0")  # migrations/1.1 registers it for a database that old
        self.assertEqual(self._registries().env_file_path, SCRATCH_OLD_KEY_FILE)
        self.assertFalse(os.path.exists(SCRATCH_KEY_FILE))

        migration.migrate(self.env.cr, "1.1")
        registry = self._registries()
        self.assertEqual((registry.name, registry.env_file_path, registry.os_group), ("Hand Made Safety Row", SCRATCH_KEY_FILE, SCRATCH_GROUP))
        self.assertTrue(os.path.exists(SCRATCH_KEY_FILE))
