# Copyright © Bruce Perens K6BP.
# SPDX-License-Identifier: AGPL-3.0-or-later
import importlib.util
import os

from odoo.addons.zero_sudo.tests.common import HamsTransactionCase
from odoo.tests.common import tagged

MIGRATION = os.path.join(os.path.dirname(__file__), "..", "migrations", "1.1", "pre-migration.py")


def _load_migration():
    spec = importlib.util.spec_from_file_location("database_management_migration_1_1", MIGRATION)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


@tagged("post_install", "-at_install")
class TestMigration11(HamsTransactionCase):
    """cron_check_bloat has been in a noupdate="0" block since 2026-09-28 so an upgrade would give it its group, but a database
    that first loaded it inside the old noupdate="1" block keeps `ir_model_data.noupdate = true`, which Odoo checks again, so the
    group never reached the row and the job was refused every day. The 1.1 pre-migration clears that flag."""

    def _flag(self):
        self.env.cr.execute(
            "SELECT noupdate FROM ir_model_data WHERE module = 'database_management' AND name = 'cron_check_bloat'"
        )
        return self.env.cr.fetchone()[0]

    def test_migration_1_1_clears_the_stale_noupdate_flag_of_the_bloat_cron(self):
        self.env.cr.execute(
            "UPDATE ir_model_data SET noupdate = TRUE WHERE module = 'database_management' AND name = 'cron_check_bloat'"
        )
        self.assertTrue(self._flag())
        _load_migration().migrate(self.env.cr, "1.0")
        self.assertFalse(self._flag())
        _load_migration().migrate(self.env.cr, "1.0")
        self.assertFalse(self._flag())

    def test_migration_1_1_leaves_a_fresh_install_alone(self):
        self.env.cr.execute(
            "UPDATE ir_model_data SET noupdate = TRUE WHERE module = 'database_management' AND name = 'cron_check_bloat'"
        )
        _load_migration().migrate(self.env.cr, None)
        self.assertTrue(self._flag())
