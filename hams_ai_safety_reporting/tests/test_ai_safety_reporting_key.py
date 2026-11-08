# Copyright © Bruce Perens K6BP.
# SPDX-License-Identifier: AGPL-3.0-or-later
"""The AI safety reporting service account gets a daemon key through the Daemon Key Manager.

The Simulated Band bots and the Official Observer file a safety ticket with a key kept in
``hooks.KEY_FILE``. These tests run the registration for real (the registry row, the API key and the
key file are all real) against a scratch file name inside the real key directory, so a developer box's
own key file is never overwritten.
"""
import os
import stat

from odoo.tests import tagged

from odoo.addons.hams_ai_safety_reporting import hooks
from odoo.addons.zero_sudo.tests.real_transaction import RealTransactionCase

SCRATCH_KEY_FILE = "/opt/hams/etc/keys/test_ai_safety_reporting_key.env"


@tagged("post_install", "-at_install", "security")
class TestAiSafetyReportingKey(RealTransactionCase):
    def setUp(self):
        super().setUp()
        self.safe_patch_object(hooks, "KEY_FILE", new=SCRATCH_KEY_FILE)
        self.addCleanup(self._remove_scratch_key_file)
        self._remove_scratch_key_file()
        self.addCleanup(lambda: self._registries().unlink())

    def _remove_scratch_key_file(self):
        if os.path.lexists(SCRATCH_KEY_FILE):
            os.remove(SCRATCH_KEY_FILE)

    def _registries(self):
        manager = self.env.ref("daemon_key_manager.user_daemon_key_manager_service")
        return self.env["daemon.key.registry"].with_user(manager).search(
            [("env_file_path", "=", SCRATCH_KEY_FILE)]
        )

    def test_registering_issues_the_key_for_the_create_only_account_and_is_idempotent(self):
        # [@ANCHOR: hams_ai_safety_reporting:test_register_ai_safety_reporting_key]
        # Tests [@ANCHOR: hams_ai_safety_reporting:register_ai_safety_reporting_key]
        service_uid = self.env["zero_sudo.security.utils"]._get_service_uid(hooks.SERVICE_XMLID)

        hooks.post_init_hook(self.env)
        registry = self._registries()
        self.assertEqual(len(registry), 1, "one registry row for the safety reporting key")
        self.assertEqual(registry.name, hooks.KEY_NAME)
        self.assertEqual(registry.user_id.id, service_uid)
        self.assertFalse(registry.os_group, "the units still run as odoo: owner-only 0600")
        self.assertEqual(stat.S_IMODE(os.stat(SCRATCH_KEY_FILE).st_mode), 0o600)
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
                "env_file_path": SCRATCH_KEY_FILE,
            }
        )

        hooks.register_ai_safety_reporting_key(self.env)

        registry = self._registries()
        self.assertEqual(len(registry), 1)
        self.assertEqual(registry.name, "Hand Made Safety Row")
        self.assertTrue(os.path.exists(SCRATCH_KEY_FILE), "the key was minted and written")
