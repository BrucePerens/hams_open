# -*- coding: utf-8 -*-
# Copyright © Bruce Perens K6BP.
# SPDX-License-Identifier: AGPL-3.0-or-later
import importlib.util
import os

from odoo.addons.zero_sudo.tests.common import HamsTransactionCase
from odoo.tests.common import tagged

MIGRATION_PATH = os.path.join(
    os.path.dirname(os.path.dirname(os.path.abspath(__file__))),
    "migrations", "1.1", "post-remove-truncate-crons.py",
)


def _load_migration():
    # Load the real migration file, the same one Odoo's upgrade runs.
    spec = importlib.util.spec_from_file_location("ses_webhook_post_migration_1_1", MIGRATION_PATH)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


@tagged("post_install", "-at_install")
class TestSesWebhookPurgeCronMigration(HamsTransactionCase):
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
            "group_ids": [(6, 0, [self.env["res.groups"].create({"name": "Purge fixture group " + xmlid_name}).id])],
        })
        for name, model, res_id in (
            (xmlid_name, "ir.cron", cron.id),
            (xmlid_name + "_ir_actions_server", "ir.actions.server", cron.ir_actions_server_id.id),
        ):
            self.env["ir.model.data"].create({
                "module": "ses_webhook", "name": name, "model": model, "res_id": res_id, "noupdate": True,
            })
        # What a cron that has been running for months carries: a progress row and a queued trigger.
        self.env.cr.execute(  # audit-ignore-sql: test fixture, bound params
            "INSERT INTO ir_cron_progress (cron_id, remaining, done, timed_out_counter)"
            " VALUES (%s, 0, 0, 0)", (cron.id,)
        )
        self.env.cr.execute(  # audit-ignore-sql: test fixture, bound params
            "INSERT INTO ir_cron_trigger (cron_id, call_at) VALUES (%s, now())", (cron.id,)
        )
        return cron

    def test_01_migration_unlinks_the_purge_crons_by_xmlid(self):
        migration = _load_migration()
        crons = self.env["ir.cron"].browse()
        for name in migration.CRON_XMLIDS:
            crons |= self._make_cron(name, "ses.webhook.log")
        action_ids = crons.ir_actions_server_id.ids
        keeper = self._make_cron("kept_unrelated_cron", "ses.webhook.log")
        self.env.flush_all()
        for _ in range(2):  # second run proves idempotence
            migration.migrate(self.env.cr, "1.0")
            self.env.invalidate_all()
            self.assertFalse(crons.exists())
            self.assertFalse(self.env["ir.actions.server"].browse(action_ids).exists())
            for name in migration.CRON_XMLIDS + migration.SERVER_ACTION_XMLIDS:
                self.assertFalse(
                    self.env.ref("ses_webhook." + name, raise_if_not_found=False)
                )
            self.assertTrue(keeper.exists(), "an unrelated cron is left alone")

    def test_02_migration_skips_a_fresh_install(self):
        migration = _load_migration()
        cron = self._make_cron(migration.CRON_XMLIDS[0], "ses.webhook.log")
        migration.migrate(self.env.cr, None)  # no previous version: nothing to retire
        self.assertTrue(cron.exists())

    def test_03_no_purge_cron_xmlid_remains_on_an_upgraded_database(self):
        migration = _load_migration()
        names = migration.CRON_XMLIDS + migration.SERVER_ACTION_XMLIDS
        self.assertFalse(
            self.env["ir.model.data"].search_count(
                [("module", "=", migration.MODULE), ("name", "in", list(names))]
            ),
            "these must not exist on a database that has run the 1.1 migration: " + ", ".join(names),
        )

    def test_04_no_truncate_methods_and_service_account_cannot_delete(self):
        log = self.env["ses.webhook.log"]
        pending = self.env["ses.webhook.pending_submission"]
        self.assertNotIn("_cron_truncate_logs", dir(log))
        self.assertNotIn("_cron_truncate_pending_submissions", dir(pending))
        svc = self.env.ref("ses_webhook.user_ses_webhook_service_internal")
        for model in (log, pending):
            self.assertFalse(model.with_user(svc).has_access("unlink"))
