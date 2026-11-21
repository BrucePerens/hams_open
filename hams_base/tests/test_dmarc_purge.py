# -*- coding: utf-8 -*-
"""The DMARC report purge cron deletes old reports and keeps recent ones."""
from datetime import timedelta

from odoo import fields
from odoo.tests import common, tagged


@tagged("post_install", "-at_install")
class TestDmarcPurge(common.TransactionCase):
    # [@ANCHOR: hams_base:test_dmarc_purge_drops_only_old_reports]
    def test_dmarc_purge_drops_only_old_reports(self):
        # Tests [@ANCHOR: hams_base:dmarc_report_purge]
        # Tests [@ANCHOR: hams_base:dmarc_purge_service_account]
        Report = self.env["hams_base.dmarc.report"]
        old = Report.create({"org_name": "purge.old", "report_id": "o"})
        new = Report.create({"org_name": "purge.new", "report_id": "n"})
        self.env["hams_base.dmarc.record"].create({"report_id": old.id, "source_ip": "192.0.2.1"})
        self.env.cr.execute(
            "UPDATE hams_base_dmarc_report SET create_date = %s WHERE id = %s",
            (fields.Datetime.now() - timedelta(days=400), old.id),
        )
        Report.invalidate_model()
        service = self.env.ref("hams_base.user_dmarc_purge_service")
        self.assertEqual(Report.with_user(service)._cron_purge_old_reports(), 1)
        self.assertFalse(old.exists())
        self.assertTrue(new.exists())
        self.assertFalse(self.env["hams_base.dmarc.record"].search([("source_ip", "=", "192.0.2.1")]))
        self.assertTrue(self.env.ref("hams_base.ir_cron_dmarc_purge").active)
