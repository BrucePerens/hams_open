# -*- coding: utf-8 -*-
# Part of Odoo. See LICENSE file for full copyright and licensing details.
#
# This file is part of hams_open, an open source module.
# License: AGPL-3.0

import os

from odoo.modules.module import get_module_path
from odoo.tests.common import tagged

from . import common


@tagged("post_install", "-at_install")
class TestCronServiceAccess(common.HamsTransactionCase):
    """Odoo 19's ir.actions.server._can_execute_action_on_records refuses to run an action for a user who lacks write
    access to the action's model unless the action names a group the user is in. A cron that runs as a narrow service
    account (read and delete only) is therefore refused on every run, silently, apart from a log line: four crons on
    hams.com failed daily for a week (SES webhook log and pending-submission truncation, the autovacuum bloat check, and
    the communications-consent adulthood transition) and never ran. Their own unit tests call the model methods
    directly, so nothing caught it."""

    # [@ANCHOR: test_every_cron_of_this_repository_is_allowed_to_run_as_its_own_user]
    def test_every_cron_of_this_repository_is_allowed_to_run_as_its_own_user(self):
        repo_dir = os.path.dirname(get_module_path("zero_sudo"))
        cron_data = self.env["ir.model.data"].search([("model", "=", "ir.cron")], limit=2000)
        crons = self.env["ir.cron"].browse([row.res_id for row in cron_data if self._own_module(row.module, repo_dir)])
        self.assertTrue(crons, "expected at least one cron from this repository's modules")
        refused = []
        for cron in crons.with_context(active_test=False):
            server_action = cron.ir_actions_server_id
            user = cron.user_id
            if user.id == 1 or user.has_group("base.group_system") and not user.is_service_account:
                continue
            model = server_action.model_id.model
            if server_action.group_ids:
                allowed = bool(server_action.group_ids & user.all_group_ids)
            else:
                allowed = self.env[model].with_user(user).has_access("write")
            if not allowed:
                refused.append(f"{cron.cron_name} (as {user.login} on {model})")
        self.assertEqual(refused, [], "these crons would be refused by Odoo before they run: " + "; ".join(refused))

    def _own_module(self, module, repo_dir):
        path = get_module_path(module, display_warning=False)
        return bool(path) and os.path.dirname(path) == repo_dir
