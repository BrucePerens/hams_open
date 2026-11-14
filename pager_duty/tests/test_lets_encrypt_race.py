# Copyright © Bruce Perens K6BP.
# SPDX-License-Identifier: AGPL-3.0-or-later

# -*- coding: utf-8 -*-
"""Two simultaneous first update_lets_encrypt_domains calls must not both create the certbot check.

Two real cursors: the second one's REPEATABLE READ snapshot is taken before the first one commits, so it cannot see the first
one's check and, without the lock, would create a second (NULL website_id is not covered by UNIQUE)."""
import odoo.api
from psycopg2 import errors
from odoo.tests import tagged
from odoo.addons.zero_sudo.tests.real_transaction import RealTransactionCase


@tagged("post_install", "-at_install")
class TestLetsEncryptRace(RealTransactionCase):
    def test_a_simultaneous_first_update_does_not_duplicate(self):
        # Tests [@ANCHOR: pager_duty:update_lets_encrypt_domains]
        self.safe_patch_object(type(self.env["pager.check"]), "action_push_to_json")
        self.env["pager.check"].search([("check_type", "=", "certbot")]).unlink()
        self.env.cr.commit()
        later_cr = self.registry.cursor()
        self.addCleanup(later_cr.close)
        later = odoo.api.Environment(later_cr, self.env.uid, {})
        later_cr.execute("SELECT 1")  # the snapshot is taken now, before the first call commits

        self.env["pager.check"].update_lets_encrypt_domains(["example.org"])
        self.env.cr.commit()

        with self.assertRaises(errors.SerializationFailure):
            later["pager.check"].update_lets_encrypt_domains(["example.net"])
        later_cr.rollback()
        self.assertEqual(self.env["pager.check"].search_count([("check_type", "=", "certbot")]), 1)
