# Copyright © Bruce Perens K6BP.
# SPDX-License-Identifier: AGPL-3.0-or-later

# -*- coding: utf-8 -*-
"""Two simultaneous autodiscoveries must not both create the baseline checks.

Two real cursors: the second one's REPEATABLE READ snapshot is taken before the first one commits, so it cannot see the first
one's checks and, without the lock, would create every one of them again (NULL website_id is not covered by UNIQUE)."""
from unittest.mock import MagicMock

import odoo.api
from psycopg2 import errors
from odoo.tests import tagged
from odoo.addons.zero_sudo.tests.real_transaction import RealTransactionCase


@tagged("post_install", "-at_install")
class TestAutodiscoveryRace(RealTransactionCase):
    def test_a_simultaneous_autodiscovery_does_not_duplicate(self):
        # Tests [@ANCHOR: pager_duty:run_autodiscovery]
        self.safe_patch_object(type(self.env["pager.check"]), "action_push_to_json")
        listing = MagicMock()
        listing.stdout = "postgresql.service"
        self.safe_patch("odoo.addons.pager_duty.models.pager_check.subprocess.run", return_value=listing)
        later_cr = self.registry.cursor()
        self.addCleanup(later_cr.close)
        later = odoo.api.Environment(later_cr, self.env.uid, {})
        later_cr.execute("SELECT 1")  # the snapshot is taken now, before the first run commits

        self.env["pager.check"]._run_autodiscovery()
        self.env.cr.commit()

        with self.assertRaises(errors.SerializationFailure):
            later["pager.check"]._run_autodiscovery()
        later_cr.rollback()
        names = self.env["pager.check"].search([("comment", "ilike", "Autodiscovered")]).mapped("name")
        self.assertEqual(len(names), len(set(names)))
