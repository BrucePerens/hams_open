# -*- coding: utf-8 -*-
# Copyright © Bruce Perens K6BP. All Rights Reserved.
# This software is proprietary and confidential.
"""Two real database connections: an image upload whose snapshot predates another upload to the same site (REPEATABLE READ
cannot see the other's commit, so a count alone lets both pass the cap)."""
import odoo
from psycopg2 import errors
from odoo.addons.zero_sudo.tests.real_transaction import RealTransactionCase
from odoo.tests.common import tagged
from odoo.tools import mute_logger

from ..controllers import owner_site


@tagged("post_install", "-at_install")
class TestImageCapRace(RealTransactionCase):
    # [@ANCHOR: user_websites:test_image_cap_race]
    def test_a_simultaneous_upload_cannot_pass_the_cap_on_a_stale_snapshot(self):
        # Tests [@ANCHOR: user_websites:owner_create_image_within_cap]
        user = self.env["res.users"].create({"name": "Image Race", "login": "image_race_member"})
        self.env.cr.commit()
        self.addCleanup(self._delete, user.id)
        self.safe_patch_object(owner_site, "MAX_IMAGES_PER_SITE", 1)

        svc_uid = self.env["zero_sudo.security.utils"]._get_service_uid(owner_site.SVC)

        def vals(name):
            return {"name": name, "datas": "UklGRg==", "res_model": "res.users", "res_id": user.id,
                    "description": owner_site.IMAGE_DESCRIPTION}

        with self.registry.cursor() as loser_cr:
            loser = odoo.api.Environment(loser_cr, svc_uid, {})["ir.attachment"]
            loser.search_count([("res_id", "=", user.id)])  # the loser's snapshot starts here
            with self.registry.cursor() as winner_cr:
                winner = odoo.api.Environment(winner_cr, svc_uid, {})["ir.attachment"]
                self.assertTrue(owner_site._create_image_within_cap(winner, "res.users", user.id, vals("a")))
            with mute_logger("odoo.sql_db"), self.assertRaises(errors.SerializationFailure):
                owner_site._create_image_within_cap(loser, "res.users", user.id, vals("b"))

    def _delete(self, user_id):
        with self.registry.cursor() as cr:
            cr.execute("DELETE FROM ir_attachment WHERE res_model = 'res.users' AND res_id = %s", (user_id,))
            cr.execute("SELECT partner_id FROM res_users WHERE id = %s", (user_id,))
            partner_id = cr.fetchone()[0]
            cr.execute("DELETE FROM res_users WHERE id = %s", (user_id,))
            cr.execute("DELETE FROM res_partner WHERE id = %s", (partner_id,))
