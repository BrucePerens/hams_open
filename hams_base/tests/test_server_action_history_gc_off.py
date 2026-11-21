# -*- coding: utf-8 -*-
# Copyright © Bruce Perens K6BP. License: AGPL-3.0.
from odoo.addons.base.models.ir_autovacuum import is_autovacuum
from odoo.addons.zero_sudo.tests.common import HamsTransactionCase
from odoo.tests import tagged


@tagged("post_install", "-at_install")
class TestServerActionHistoryGcOff(HamsTransactionCase):
    # [@ANCHOR: hams_base:test_server_action_history_gc_off]
    def test_old_server_action_revisions_survive_the_gc(self):
        # Tests [@ANCHOR: hams_base_ir_actions_server_history_ir_actions_server_history]
        # Tests [@ANCHOR: hams_base:server_action_history_gc_off]
        model = self.env["ir.actions.server.history"]
        action = self.env["ir.actions.server"].create(
            {
                "name": "history kept",
                "model_id": self.env.ref("base.model_res_partner").id,
                "state": "code",
                "code": "pass",
            }
        )
        count = model._max_entries_per_action + 5
        revisions = model.create([{"action_id": action.id, "code": f"x = {n}"} for n in range(count)])
        model._gc_histories()
        self.assertEqual(
            len(revisions.exists()), count, "[!] DIAGNOSTIC FOR AI: server action history is stored information and must never be purged."
        )
        self.assertFalse(is_autovacuum(type(model)._gc_histories), "the vacuum must not run it")
