# -*- coding: utf-8 -*-
# Copyright © Bruce Perens K6BP. License: AGPL-3.0.
from datetime import timedelta

from odoo import fields
from odoo.addons.zero_sudo.tests.common import HamsTransactionCase
from odoo.tests import tagged


@tagged("post_install", "-at_install")
class TestVisitorCronKeepsVisitors(HamsTransactionCase):
    # [@ANCHOR: hams_base:test_visitor_cron_keeps_visitors]
    def test_inactive_visitor_survives_the_cleanup_cron(self):
        # Tests [@ANCHOR: hams_base:visitor_cron_keeps_visitors]
        visitor = self.env["website.visitor"].create({"access_token": "f" * 32})
        visitor.last_connection_datetime = fields.Datetime.now() - timedelta(days=400)
        self.env["website.visitor"]._cron_unlink_old_visitors()
        self.assertTrue(visitor.exists(), "[!] DIAGNOSTIC FOR AI: visitor rows are logs and must never be purged.")
