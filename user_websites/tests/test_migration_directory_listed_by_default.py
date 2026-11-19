# Copyright © Bruce Perens K6BP.
# SPDX-License-Identifier: AGPL-3.0-or-later
import importlib.util
import os

from odoo.addons.zero_sudo.tests.common import HamsTransactionCase
from odoo.tests.common import tagged

MODULE_DIR = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))


def _migration():
    path = os.path.join(MODULE_DIR, "migrations", "0.11", "pre-migration.py")
    spec = importlib.util.spec_from_file_location("user_websites_pre_migration_0_11", path)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


@tagged("post_install", "-at_install")
class TestMigrationDirectoryListedByDefault(HamsTransactionCase):
    # Tests [@ANCHOR: user_websites:migrate_directory_listed_by_default]
    # [@ANCHOR: test_migration_directory_listed_by_default]
    def _user(self, login, **vals):
        return self.env["res.users"].create({"name": login, "login": login, **vals})

    def test_01_new_user_is_listed_by_default(self):
        self.assertTrue(self._user("dirdefault").privacy_show_in_directory)

    def test_02_migration_lists_existing_members(self):
        active = self._user("dirmigactive")
        suspended = self._user("dirmigsusp", is_suspended_from_websites=True)
        pending = self._user("dirmigpend", gdpr_erasure_pending=True)
        for user in (active, suspended, pending):
            self.env.cr.execute(  # audit-ignore-sql: static SQL; id is an integer
                "UPDATE res_users SET privacy_show_in_directory = false WHERE id = %s", (user.id,)
            )
        migration = _migration()
        migration.migrate(self.env.cr, "19.0.0.10")
        migration.migrate(self.env.cr, "19.0.0.10")  # idempotent
        (active | suspended | pending).invalidate_recordset(["privacy_show_in_directory"])
        self.assertTrue(active.privacy_show_in_directory)
        self.assertTrue(suspended.privacy_show_in_directory, "suspension is excluded by the view, not the flag")
        self.assertFalse(pending.privacy_show_in_directory, "a pending erasure stays unlisted")

    def test_03_suspended_user_not_in_directory_view(self):
        suspended = self._user("dirviewsusp", website_slug="dirviewsusp", is_suspended_from_websites=True)
        self.env.cr.execute(  # audit-ignore-sql: static SQL; id is an integer
            "SELECT 1 FROM user_websites_public_directory_view WHERE user_id = %s", (suspended.id,)
        )
        self.assertFalse(self.env.cr.fetchone())

    def test_04_migration_does_nothing_on_fresh_install(self):
        user = self._user("dirmigfresh")
        self.env.cr.execute(  # audit-ignore-sql: static SQL; id is an integer
            "UPDATE res_users SET privacy_show_in_directory = false WHERE id = %s", (user.id,)
        )
        _migration().migrate(self.env.cr, None)
        user.invalidate_recordset(["privacy_show_in_directory"])
        self.assertFalse(user.privacy_show_in_directory)
