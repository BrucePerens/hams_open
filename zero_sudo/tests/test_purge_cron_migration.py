# -*- coding: utf-8 -*-
# Copyright © Bruce Perens K6BP.
# SPDX-License-Identifier: AGPL-3.0-or-later
import importlib.util
import os

from odoo.addons.zero_sudo.tests.common import HamsTransactionCase
from odoo.tests.common import tagged

MIGRATION_PATH = os.path.join(
    os.path.dirname(os.path.dirname(os.path.abspath(__file__))),
    "migrations", "1.1", "post-remove-security-log-autovacuum.py",
)


def _load_migration():
    # Load the real migration file, the same one Odoo's upgrade runs.
    spec = importlib.util.spec_from_file_location("zero_sudo_post_migration_1_1", MIGRATION_PATH)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


@tagged("post_install", "-at_install")
class TestPurgeCronMigration(HamsTransactionCase):
    """Bruce, 2026-10-11: "We retain data forever." The deletion crons are gone: the 1.1
    migration unlinks the ones an installed database still holds, by xmlid."""

    def _make_cron(self, xmlid_name, model_name):
        cron = self.env["ir.cron"].create({
            "name": "Retired purge fixture " + xmlid_name,
            "model_id": self.env["ir.model"]._get_id(model_name),
            "state": "code",
            "code": "pass",
            "interval_number": 1,
            "interval_type": "days",
            "user_id": self.env.ref("base.user_root").id,
        })
        self.env["ir.model.data"].create({
            "module": "zero_sudo", "name": xmlid_name, "model": "ir.cron",
            "res_id": cron.id, "noupdate": True,
        })
        return cron

    def test_01_migration_unlinks_the_purge_crons_by_xmlid(self):
        migration = _load_migration()
        crons = self.env["ir.cron"].browse()
        for name in migration.CRON_XMLIDS:
            crons |= self._make_cron(name, "zero_sudo.security.log")
        action_ids = crons.ir_actions_server_id.ids
        keeper = self._make_cron("kept_unrelated_cron", "zero_sudo.security.log")
        self.env.flush_all()
        for _ in range(2):  # second run proves idempotence
            migration.migrate(self.env.cr, "1.0")
            self.env.invalidate_all()
            self.assertFalse(crons.exists())
            self.assertFalse(self.env["ir.actions.server"].browse(action_ids).exists())
            for name in migration.CRON_XMLIDS:
                self.assertFalse(
                    self.env.ref("zero_sudo." + name, raise_if_not_found=False)
                )
            self.assertTrue(keeper.exists(), "an unrelated cron is left alone")

    def test_02_migration_skips_a_fresh_install(self):
        migration = _load_migration()
        cron = self._make_cron(migration.CRON_XMLIDS[0], "zero_sudo.security.log")
        migration.migrate(self.env.cr, None)  # no previous version: nothing to retire
        self.assertTrue(cron.exists())

    def test_03_no_purge_cron_xmlid_remains_on_an_upgraded_database(self):
        migration = _load_migration()
        for name in migration.CRON_XMLIDS:
            self.assertFalse(
                self.env["ir.model.data"].search_count(
                    [("module", "=", "zero_sudo"), ("name", "=", name)]
                ),
                name + " must not exist on a database that has run the 1.1 migration",
            )

    def test_04_security_log_has_no_autovacuum(self):
        self.assertFalse(hasattr(self.env["zero_sudo.security.log"], "autovacuum"))
