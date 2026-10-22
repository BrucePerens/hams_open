# Copyright © Bruce Perens K6BP.
# SPDX-License-Identifier: AGPL-3.0-or-later
# -*- coding: utf-8 -*-
"""The exception reporter service account gets a daemon key through the Daemon Key Manager.

The OnFailure reporter and a daemon's ``reporting_failures`` authenticate as the exception reporter
service account with a key kept in ``hooks.KEY_FILE``. These tests run the registration for real
(the registry row, the API key and the key file are all real) against a scratch file name inside
the real key directory, so a developer box's own reporter key file is never overwritten.
"""
import os
import stat

from odoo.tests import tagged

from odoo.addons.hams_helpdesk import hooks
from odoo.addons.zero_sudo.tests.real_transaction import RealTransactionCase

SCRATCH_KEY_FILE = "/opt/hams/etc/keys/test_exception_reporter_key.env"


@tagged("post_install", "-at_install")
class TestExceptionReporterKey(RealTransactionCase):
    def setUp(self):
        super().setUp()
        self.safe_patch_object(hooks, "KEY_FILE", new=SCRATCH_KEY_FILE)
        self.addCleanup(self._remove_scratch_key_file)
        self._remove_scratch_key_file()

    def _remove_scratch_key_file(self):
        if os.path.lexists(SCRATCH_KEY_FILE):
            os.remove(SCRATCH_KEY_FILE)

    def _registries(self):
        return self.env["daemon.key.registry"].search([("env_file_path", "=", SCRATCH_KEY_FILE)])

    def test_registering_issues_the_key_for_the_reporter_account_and_is_idempotent(self):
        # [@ANCHOR: hams_helpdesk:test_register_exception_reporter_key]
        # Tests [@ANCHOR: hams_helpdesk:register_exception_reporter_key]
        service_uid = self.env["zero_sudo.security.utils"]._get_service_uid(hooks.SERVICE_XMLID)
        self.addCleanup(lambda: self._registries().unlink())

        hooks.register_exception_reporter_key(self.env)
        registry = self._registries()
        self.assertEqual(len(registry), 1, "one registry row for the reporter key")
        self.assertEqual(registry.user_id.id, service_uid)
        self.assertTrue(os.path.exists(SCRATCH_KEY_FILE), "the key file was written")
        self.assertEqual(stat.S_IMODE(os.stat(SCRATCH_KEY_FILE).st_mode), 0o600)
        with open(SCRATCH_KEY_FILE, encoding="utf-8") as key_file:
            content = key_file.read()
        self.assertIn("ODOO_RPC_LOGIN=" + registry.user_id.login, content)
        self.assertIn("ODOO_RPC_KEY=", content)

        # Registering again (a re-install, the migration on an installed database) updates the same row.
        hooks.register_exception_reporter_key(self.env)
        self.assertEqual(self._registries(), registry)
