# -*- coding: utf-8 -*-
# Part of Odoo. See LICENSE file for full copyright and licensing details.
#
# This file is part of hams_open, an open source module.
# License: AGPL-3.0

from odoo.tests.common import tagged

from . import common
from . import test_persona_support as personas


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
        # zero_sudo and ses_webhook no longer ship a cron (their purge jobs were removed); a database with none of
        # this repository's cron-carrying modules installed has no cron to check, and the sweep checks nothing.
        # The gate is the one Odoo calls (personas.cron_refusal); superuser crons are checked too, and pass it.
        refused = personas.cron_sweep(self.env, "zero_sudo")
        self.assertEqual(refused, [], "these crons would be refused by Odoo before they run: " + "; ".join(refused))
