# -*- coding: utf-8 -*-
# Copyright © Bruce Perens K6BP. License: AGPL-3.0.
from odoo import fields
from odoo.addons.zero_sudo.tests.common import HamsTransactionCase
from odoo.tests import tagged


@tagged("post_install", "-at_install")
class TestUserLogGcOff(HamsTransactionCase):
    # [@ANCHOR: hams_base:test_user_log_gc_off]
    def test_older_login_records_survive_the_gc(self):
        # Tests [@ANCHOR: hams_base_res_users_log_res_users_log]
        # Tests [@ANCHOR: hams_base:user_log_gc_off]
        model = self.env["res.users.log"]
        older, newer = model.create([{}, {}])
        # audit-ignore-sql: bound parameters; the test back-dates one login record
        self.env.cr.execute("UPDATE res_users_log SET create_date = %s WHERE id = %s", (fields.Datetime.now().replace(year=2020), older.id))
        model.invalidate_model()
        model._gc_user_logs()
        self.assertEqual(len((older | newer).exists()), 2, "[!] DIAGNOSTIC FOR AI: login records are logs and must never be purged.")
        self.assertNotIn("_autovacuum", type(model)._gc_user_logs.__dict__, "the vacuum must not run it")
