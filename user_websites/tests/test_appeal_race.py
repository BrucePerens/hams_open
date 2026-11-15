# -*- coding: utf-8 -*-
# Copyright © Bruce Perens K6BP. All Rights Reserved.
# This software is proprietary and confidential.
"""Two real database connections: an appeal whose snapshot predates another appeal of the same member (REPEATABLE READ cannot
see the other's commit, so a count alone lets both create one)."""
import odoo
from psycopg2 import errors
from odoo.addons.zero_sudo.tests.real_transaction import RealTransactionCase
from odoo.tests.common import tagged
from odoo.tools import mute_logger


@tagged("post_install", "-at_install")
class TestAppealRace(RealTransactionCase):
    # [@ANCHOR: user_websites:test_appeal_race]
    def test_a_simultaneous_appeal_cannot_pass_the_one_pending_rule_on_a_stale_snapshot(self):
        # Tests [@ANCHOR: user_websites:appeal_create_if_none_open]
        user = self.env["res.users"].create({"name": "Appeal Race", "login": "appeal_race_member"})
        svc_uid = self.env["zero_sudo.security.utils"]._get_service_uid("user_websites.user_websites_service_account")
        self.env.cr.commit()
        self.addCleanup(self._delete, user.id)
        vals = {"user_id": user.id, "reason": "r", "state": "new"}
        with self.registry.cursor() as loser_cr:
            loser = odoo.api.Environment(loser_cr, svc_uid, {})["content.violation.appeal"]
            loser.search_count([("user_id", "=", user.id)])  # the loser's snapshot starts here
            with self.registry.cursor() as winner_cr:
                winner = odoo.api.Environment(winner_cr, svc_uid, {})["content.violation.appeal"]
                self.assertTrue(winner._create_if_none_open(vals))
                self.assertFalse(winner._create_if_none_open(vals))
            with mute_logger("odoo.sql_db"), self.assertRaises(errors.SerializationFailure):
                loser._create_if_none_open(vals)

    def _delete(self, user_id):
        with self.registry.cursor() as cr:
            cr.execute("DELETE FROM content_violation_appeal WHERE user_id = %s", (user_id,))
            cr.execute("SELECT partner_id FROM res_users WHERE id = %s", (user_id,))
            partner_id = cr.fetchone()[0]
            cr.execute("DELETE FROM res_users WHERE id = %s", (user_id,))
            cr.execute("DELETE FROM res_partner WHERE id = %s", (partner_id,))
