# Copyright © Bruce Perens K6BP.
# SPDX-License-Identifier: AGPL-3.0-or-later

# -*- coding: utf-8 -*-
"""unregister_daemon(): a module that stops using a daemon key removes its registry row and revokes the key.

Real registry rows, real API keys and a real key file in the real key directory (under a scratch name)."""
import os

from odoo.exceptions import AccessError
from odoo.tests import tagged
from odoo.addons.zero_sudo.tests.real_transaction import RealTransactionCase

SCRATCH_KEY_FILE = "/opt/hams/etc/keys/unregister_test.env"
DAEMON_NAME = "Unregister Test Daemon"


@tagged("post_install", "-at_install")
class TestUnregisterDaemon(RealTransactionCase):
    def setUp(self):
        super().setUp()
        self.addCleanup(self._remove_scratch_key_file)
        self._remove_scratch_key_file()
        self.manager = self.env.ref("daemon_key_manager.user_daemon_key_manager_service")
        self.service_user = self.env["res.users"].create(
            {
                "name": "Unregister Test Service Account",
                "login": "unregister_test_service_account",
                "is_service_account": True,
            }
        )
        self.registry_model = self.env["daemon.key.registry"].with_user(self.manager)
        self.registry_model.register_daemon(
            DAEMON_NAME, self.service_user.login, SCRATCH_KEY_FILE
        )

    def _remove_scratch_key_file(self):
        if os.path.lexists(SCRATCH_KEY_FILE):
            os.remove(SCRATCH_KEY_FILE)

    def _rows(self):
        return self.registry_model.search([("name", "=", DAEMON_NAME)])

    def _key_count(self):
        self.env.cr.execute(
            "SELECT count(*) FROM res_users_apikeys WHERE user_id = %s AND name = %s",
            (self.service_user.id, f"{DAEMON_NAME}_key"),
        )
        return self.env.cr.fetchone()[0]

    def test_a_manager_removes_the_row_and_revokes_its_key(self):
        # [@ANCHOR: daemon_key_manager:test_unregister_daemon]
        # Tests [@ANCHOR: daemon_key_manager:unregister_daemon]
        self.assertEqual(len(self._rows()), 1)
        self.assertEqual(self._key_count(), 1)

        removed = self.registry_model.unregister_daemon(DAEMON_NAME)

        self.assertEqual(removed, 1)
        self.assertFalse(self._rows())
        self.assertEqual(self._key_count(), 0)
        self.assertTrue(self.service_user.exists(), "the service account stays")
        self.assertTrue(os.path.exists(SCRATCH_KEY_FILE), "the key file is left for an operator")

    def test_a_second_call_removes_nothing(self):
        # Tests [@ANCHOR: daemon_key_manager:unregister_daemon]
        self.assertEqual(self.registry_model.unregister_daemon(DAEMON_NAME), 1)
        self.assertEqual(self.registry_model.unregister_daemon(DAEMON_NAME), 0)
        self.assertEqual(self.registry_model.unregister_daemon("No Such Daemon"), 0)

    def test_a_service_account_cannot_unregister_a_daemon(self):
        # Tests [@ANCHOR: daemon_key_manager:unregister_daemon]
        outsider = self.env["daemon.key.registry"].with_user(self.service_user)
        with self.assertRaises(AccessError):
            outsider.unregister_daemon(DAEMON_NAME)
        self.assertEqual(len(self._rows()), 1, "the refused call removed nothing")
