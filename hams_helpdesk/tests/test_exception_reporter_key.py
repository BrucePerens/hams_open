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
from types import SimpleNamespace

from odoo.tests import tagged

from odoo.addons.hams_helpdesk import hooks
from odoo.addons.zero_sudo.tests.real_transaction import RealTransactionCase

SCRATCH_KEY_FILE = "/opt/hams/etc/keys/test_exception_reporter_key.env"


@tagged("post_install", "-at_install")
class TestExceptionReporterKey(RealTransactionCase):
    def setUp(self):
        super().setUp()
        # No such account on this host: the flat (legacy) target, as on a host that was not provisioned.
        self.safe_patch_object(hooks, "OS_GROUP", new="hamsd_no_such_group_for_test")
        self.safe_patch_object(hooks, "LEGACY_KEY_FILE", new=SCRATCH_KEY_FILE)
        self.addCleanup(self._remove_scratch_key_file)
        self._remove_scratch_key_file()
        # This test database persists between runs and may hold the reporter row under the family directory with its group (a provisioned
        # host registered it at install); registering the scratch path would then fail the group-directory check, so start without the row.
        self.env.cr.execute(  # audit-ignore-sql: test fixture, static text
            "DELETE FROM daemon_key_registry WHERE name = 'Exception Reporter (OnFailure and daemon helper)'"
        )

    def _remove_scratch_key_file(self):
        if os.path.lexists(SCRATCH_KEY_FILE):
            os.remove(SCRATCH_KEY_FILE)

    def _registries(self):
        manager = self.env.ref("daemon_key_manager.user_daemon_key_manager_service")
        return self.env["daemon.key.registry"].with_user(manager).search([("env_file_path", "=", SCRATCH_KEY_FILE)])

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

    def test_the_post_init_hook_registers_the_key_on_a_fresh_install(self):
        # Tests [@ANCHOR: hams_helpdesk:post_init_hook]
        self.addCleanup(lambda: self._registries().unlink())
        hooks.post_init_hook(self.env)
        self.assertEqual(len(self._registries()), 1)
        self.assertTrue(os.path.exists(SCRATCH_KEY_FILE))


@tagged("post_install", "-at_install")
class TestExceptionReporterKeyTarget(RealTransactionCase):
    def test_exception_reporter_key_target(self):
        # [@ANCHOR: hams_helpdesk:test_exception_reporter_key_target]
        # Tests [@ANCHOR: hams_helpdesk:exception_reporter_key_target]
        self.assertEqual(hooks.KEY_FILE, "/opt/hams/etc/keys/exception_report/exception_reporter.env")
        self.safe_patch_object(hooks.grp, "getgrall", return_value=[SimpleNamespace(gr_name="hamsd_exception_report")])
        self.assertEqual(hooks.exception_reporter_key_target(), (hooks.KEY_FILE, "hamsd_exception_report"))
        self.safe_patch_object(hooks.grp, "getgrall", return_value=[])
        self.assertEqual(hooks.exception_reporter_key_target(), (hooks.LEGACY_KEY_FILE, None))
