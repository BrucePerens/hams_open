# Copyright © Bruce Perens K6BP.
# SPDX-License-Identifier: AGPL-3.0-or-later

# -*- coding: utf-8 -*-
"""Two simultaneous first reports of one source must not both create an incident.

Two real cursors: the second one's REPEATABLE READ snapshot is taken before the first one commits, so without the lock it
cannot see the first incident, finds none, and creates a second one for the same source."""
import odoo.api
from psycopg2 import errors
from odoo.tests import tagged
from odoo.addons.zero_sudo.tests.real_transaction import RealTransactionCase


@tagged("post_install", "-at_install")
class TestReportIncidentRace(RealTransactionCase):
    def test_a_simultaneous_first_report_does_not_duplicate(self):
        # Tests [@ANCHOR: report_incident_rate_limit]
        self.safe_patch("odoo.addons.pager_duty.models.incident.redis_pool", None)
        self.safe_patch_object(type(self.env["pager.incident"]), "_notify_on_duty")
        later_cr = self.registry.cursor()
        self.addCleanup(later_cr.close)
        later = odoo.api.Environment(later_cr, self.env.uid, {})
        later_cr.execute("SELECT 1")  # the snapshot is taken now, before the first report commits
        payload = {"source": "race_src", "severity": "high", "description": "d"}

        self.env["pager.incident"].report_incident(dict(payload))
        self.env.cr.commit()

        with self.assertRaises(errors.SerializationFailure):
            later["pager.incident"].report_incident(dict(payload))
        later_cr.rollback()
        self.assertEqual(self.env["pager.incident"].search_count([("source", "=", "race_src")]), 1)

    def test_a_repeat_report_takes_no_company_lock(self):
        # Tests [@ANCHOR: report_incident_rate_limit]
        self.safe_patch("odoo.addons.pager_duty.models.incident.redis_pool", None)
        self.safe_patch_object(type(self.env["pager.incident"]), "_notify_on_duty")
        payload = {"source": "repeat_src", "severity": "high", "description": "d"}
        self.env["pager.incident"].report_incident(dict(payload))
        self.env.cr.commit()
        holder_cr = self.registry.cursor()
        self.addCleanup(holder_cr.close)
        # Another transaction holds the company row lock, as a creator would.
        holder_cr.execute("UPDATE res_company SET id = id WHERE id = %s", [self.env.ref("base.main_company").id])
        self.env.cr.execute("SET LOCAL lock_timeout = '500ms'")
        self.env["pager.incident"].report_incident(dict(payload))  # a repeat must not wait for the lock
        self.assertEqual(self.env["pager.incident"].search([("source", "=", "repeat_src")]).occurrence_count, 2)
        with self.assertRaises(errors.LockNotAvailable):  # a creator does wait for it
            self.env["pager.incident"].report_incident({**payload, "source": "new_src"})
        holder_cr.rollback()
