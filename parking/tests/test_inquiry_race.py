# -*- coding: utf-8 -*-
# Copyright © Bruce Perens K6BP. All Rights Reserved.
# This software is proprietary and confidential.
"""Two real database connections: a for-sale inquiry whose snapshot predates another inquiry (REPEATABLE READ cannot see the
other's commit, so a count alone lets both pass the rate limit)."""
import odoo
from psycopg2 import errors
from odoo.addons.zero_sudo.tests.real_transaction import RealTransactionCase
from odoo.tests.common import tagged
from odoo.tools import mute_logger

from ..controllers import parking_api


@tagged("post_install", "-at_install")
class TestInquiryRace(RealTransactionCase):
    # [@ANCHOR: parking:COMM_test_inquiry_race]
    def test_a_simultaneous_inquiry_cannot_pass_the_limit_on_a_stale_snapshot(self):
        # Tests [@ANCHOR: parking:COMM_inquiry_rate_limited]
        svc_uid = self.env["ir.model.data"]._xmlid_to_res_id("parking.user_parking_service", raise_if_not_found=True)
        domain = self.env["parking.domain"].create({"name": "race.example", "behavior": "for_sale"})
        self.env.cr.commit()
        self.addCleanup(self._delete, domain.id)
        self.safe_patch_object(parking_api, "INQUIRY_PER_IP_HOUR", 1)
        controller = parking_api.ParkingController()
        with self.registry.cursor() as loser_cr:
            loser = odoo.api.Environment(loser_cr, svc_uid, {})
            loser["parking.inquiry"].search_count([])  # the loser's snapshot starts here
            with self.registry.cursor() as winner_cr:
                winner = odoo.api.Environment(winner_cr, svc_uid, {})
                self.assertFalse(controller._rate_limited(winner, "h"))
                winner["parking.inquiry"].create(
                    {"domain_id": domain.id, "email": "a@b.example", "message": "m", "ip_hash": "h"}
                )
            with mute_logger("odoo.sql_db"), self.assertRaises(errors.SerializationFailure):
                controller._rate_limited(loser, "h")

    def _delete(self, domain_id):
        with self.registry.cursor() as cr:
            cr.execute("DELETE FROM parking_inquiry WHERE domain_id = %s", (domain_id,))
            cr.execute("DELETE FROM parking_domain WHERE id = %s", (domain_id,))
