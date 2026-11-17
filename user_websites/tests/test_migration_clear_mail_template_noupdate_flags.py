# Copyright © Bruce Perens K6BP.
# SPDX-License-Identifier: AGPL-3.0-or-later
import importlib.util
import os
import xml.etree.ElementTree as ET

from odoo.addons.zero_sudo.tests.common import HamsTransactionCase
from odoo.tests.common import tagged

MODULE_DIR = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
CONTROL = "ir_cron_notify_pending_reports"  # stays in the noupdate block and keeps its flag


def _migration():
    path = os.path.join(MODULE_DIR, "migrations", "0.10", "pre-migration.py")
    spec = importlib.util.spec_from_file_location("user_websites_pre_migration_0_10", path)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def _declared_noupdate():
    root = ET.parse(os.path.join(MODULE_DIR, "data", "user_websites_data.xml")).getroot()
    declared = {}
    for block in root.iter("data"):
        for rec in block.findall("record"):
            declared[rec.get("id")] = block.get("noupdate") == "1" or root.get("noupdate") == "1"
    return declared


@tagged("post_install", "-at_install")
class TestMigrationClearMailTemplateNoupdateFlags(HamsTransactionCase):
    # Tests [@ANCHOR: user_websites:migrate_clear_mail_template_noupdate_flags]
    # [@ANCHOR: test_migration_clear_mail_template_noupdate_flags]
    def _flag(self, name):
        self.env.cr.execute(  # audit-ignore-sql: static SQL; name is a constant
            "SELECT noupdate FROM ir_model_data WHERE module = 'user_websites' AND name = %s", (name,)
        )
        row = self.env.cr.fetchone()
        self.assertTrue(row, f"user_websites.{name} has no ir_model_data row")
        return row[0]

    def _set_flag(self, names, value):
        self.env.cr.execute(  # audit-ignore-sql: static SQL; names are constants
            "UPDATE ir_model_data SET noupdate = %s WHERE module = 'user_websites' AND name IN %s",
            (value, tuple(names)),
        )

    def test_01_templates_are_declared_outside_noupdate_blocks(self):
        declared = _declared_noupdate()
        for name in _migration().TEMPLATE_XMLIDS:
            self.assertIn(name, declared)
            self.assertFalse(
                declared[name],
                f"[!] DIAGNOSTIC FOR AI: {name} is in a noupdate block, so -u user_websites never rewrites it on an "
                "installed database (hams_prod kept the pre-#1323 sender and body).",
            )
        self.assertTrue(declared[CONTROL])

    def test_02_migration_clears_stale_flags_and_nothing_else(self):
        migration = _migration()
        self._set_flag(migration.TEMPLATE_XMLIDS + (CONTROL,), True)
        migration.migrate(self.env.cr, "19.0.0.9")
        for name in migration.TEMPLATE_XMLIDS:
            self.assertFalse(self._flag(name), f"{name} still has noupdate=true after the 0.10 migration")
        self.assertTrue(self._flag(CONTROL), "the migration cleared a record it does not list")
        migration.migrate(self.env.cr, "19.0.0.9")  # idempotent
        self.assertFalse(self._flag(migration.TEMPLATE_XMLIDS[0]))

    def test_03_migration_does_nothing_on_fresh_install(self):
        migration = _migration()
        self._set_flag(migration.TEMPLATE_XMLIDS, True)
        migration.migrate(self.env.cr, None)
        self.assertTrue(self._flag(migration.TEMPLATE_XMLIDS[0]))
