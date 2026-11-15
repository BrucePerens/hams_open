# Copyright © Bruce Perens K6BP.
# SPDX-License-Identifier: AGPL-3.0-or-later

# -*- coding: utf-8 -*-
"""Two simultaneous deliveries of one SNS MessageId must not both be processed.

Two real cursors: the second one's REPEATABLE READ snapshot is taken before the first one commits its log, so a plain
search cannot see it. The lock on the domain row makes the second delivery fail to serialize, and Odoo runs it again."""
import odoo.api
from psycopg2 import errors
from odoo.tests import tagged
from odoo.addons.zero_sudo.tests.real_transaction import RealTransactionCase


@tagged("post_install", "-at_install")
class TestNotificationDedupeRace(RealTransactionCase):
    def test_a_simultaneous_double_delivery_is_not_processed_twice(self):
        # Tests [@ANCHOR: ses_webhook:already_handled]
        domain = self.env["ses.webhook.domain"].create({"name": "race-test.example", "secret_token": "race_secret"})
        self.env.cr.commit()
        self.addCleanup(self._remove, domain.id)
        later_cr = self.registry.cursor()
        self.addCleanup(later_cr.close)
        later = odoo.api.Environment(later_cr, self.env.uid, {})
        later_cr.execute("SELECT 1")  # the snapshot is taken now, before the first delivery commits its log

        log = self.env["ses.webhook.log"]
        self.assertFalse(log._already_handled(domain.id, "msg-1"))
        log.create({"name": "msg-1", "payload_type": "Notification", "domain_id": domain.id, "status": "success"})
        self.env.cr.commit()

        with self.assertRaises(errors.SerializationFailure):
            later["ses.webhook.log"]._already_handled(domain.id, "msg-1")
        later_cr.rollback()
        # The redelivery that Odoo runs again sees the log and is acknowledged without processing.
        self.assertTrue(self.env["ses.webhook.log"]._already_handled(domain.id, "msg-1"))
        # A failed attempt does not count as handled, and another MessageId is its own.
        self.assertFalse(self.env["ses.webhook.log"]._already_handled(domain.id, "msg-2"))

    def _remove(self, domain_id):
        self.env.cr.rollback()
        self.env.cr.execute("DELETE FROM ses_webhook_log WHERE domain_id = %s", [domain_id])
        self.env.cr.execute("DELETE FROM ses_webhook_domain WHERE id = %s", [domain_id])
        self.env.cr.commit()
