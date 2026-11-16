# -*- coding: utf-8 -*-
# Copyright © Bruce Perens K6BP. All Rights Reserved.
# This software is proprietary and confidential.
"""Two real database connections: a page create whose snapshot predates another create for the same owner (REPEATABLE READ
cannot see the other's commit, so a count alone lets both pass the quota)."""
import odoo
from psycopg2 import errors
from odoo.addons.zero_sudo.tests.real_transaction import RealTransactionCase
from odoo.tests.common import tagged
from odoo.tools import mute_logger

SVC = "user_websites.user_websites_service_account"


@tagged("post_install", "-at_install")
class TestPageQuotaRace(RealTransactionCase):
    # [@ANCHOR: user_websites:test_page_quota_race]
    def test_a_simultaneous_create_cannot_pass_the_quota_on_a_stale_snapshot(self):
        # Tests [@ANCHOR: user_websites:page_quota_lock_rows]
        user = self.env["res.users"].create(
            {"name": "Page Race", "login": "page_race_member", "website_page_limit": 1}
        )
        self.env.cr.commit()
        self.addCleanup(self._delete, user.id)
        svc_uid = self.env["zero_sudo.security.utils"]._get_service_uid(SVC)

        def vals(name):
            return {"url": f"/page-race/{name}", "name": name, "type": "qweb", "owner_user_id": user.id}

        with self.registry.cursor() as loser_cr:
            loser = odoo.api.Environment(loser_cr, svc_uid, {})["website.page"]
            loser.search_count([("owner_user_id", "=", user.id)])  # the loser's snapshot starts here
            with self.registry.cursor() as winner_cr:
                winner = odoo.api.Environment(winner_cr, svc_uid, {})["website.page"]
                self.assertTrue(winner.create(vals("a")))
            with mute_logger("odoo.sql_db"), self.assertRaises(errors.SerializationFailure):
                loser.create(vals("b"))

    def _delete(self, user_id):
        svc_uid = self.env["zero_sudo.security.utils"]._get_service_uid(SVC)
        with self.registry.cursor() as cr:
            env = odoo.api.Environment(cr, svc_uid, {})
            pages = env["website.page"].with_context(active_test=False).search([("owner_user_id", "=", user_id)])
            pages.unlink()
            cr.execute("SELECT partner_id FROM res_users WHERE id = %s", (user_id,))
            partner_id = cr.fetchone()[0]
            cr.execute("DELETE FROM res_users WHERE id = %s", (user_id,))
            cr.execute("DELETE FROM res_partner WHERE id = %s", (partner_id,))
