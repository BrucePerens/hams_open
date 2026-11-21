# Copyright © Bruce Perens K6BP.
# SPDX-License-Identifier: AGPL-3.0-or-later

# -*- coding: utf-8 -*-
import os

from odoo.exceptions import UserError
from odoo.tests import tagged
from odoo.addons.zero_sudo.tests.real_transaction import RealTransactionCase

DAEMON_NAME = "Revoke Log Test Daemon"
KEY_FILE = "/opt/hams/etc/keys/revoke_log_test.env"


@tagged("post_install", "-at_install")
class TestApikeyRevocationLog(RealTransactionCase):
    # [@ANCHOR: daemon_key_manager:test_apikey_revocation_log]
    # Tests [@ANCHOR: daemon_key_manager:apikey_revocation_log]
    def test_revoke_leaves_permanent_log_row(self):
        manager = self.env.ref("daemon_key_manager.user_daemon_key_manager_service")
        service = self.env["res.users"].create(
            {"name": "Revoke Log Service", "login": "revoke_log_service", "is_service_account": True}
        )
        registry = self.env["daemon.key.registry"].with_user(manager)
        registry.register_daemon(DAEMON_NAME, service.login, KEY_FILE)
        self.addCleanup(self._rm_key_file)
        self.env.cr.execute(
            "SELECT id FROM res_users_apikeys WHERE user_id = %s AND name = %s",
            (service.id, f"{DAEMON_NAME}_key"),
        )
        key_id = self.env.cr.fetchone()[0]

        registry.unregister_daemon(DAEMON_NAME)

        self.env.cr.execute("SELECT count(*) FROM res_users_apikeys WHERE id = %s", (key_id,))
        self.assertEqual(self.env.cr.fetchone()[0], 0)
        row = self.env["apikey.revocation.log"].with_user(manager).search([("key_id", "=", key_id)])
        self.assertEqual(len(row), 1)
        self.assertEqual(row.owner_login, "revoke_log_service")
        self.assertEqual(row.key_name, f"{DAEMON_NAME}_key")
        self.assertEqual(row.source, "admin")
        self.assertEqual(row.revoked_by_id, manager)
        self.assertTrue(row.revoked_at)
        self.assertEqual(len(row.key_fingerprint), 12)
        with self.assertRaises(UserError):
            row.unlink()
        with self.assertRaises(UserError):
            row.write({"key_name": "x"})
        self.assertTrue(row.exists())

    def _rm_key_file(self):
        if os.path.lexists(KEY_FILE):
            os.remove(KEY_FILE)
