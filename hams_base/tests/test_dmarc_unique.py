# -*- coding: utf-8 -*-
"""One DMARC report per reporting organization and report id, and the 1.1 migration removes existing duplicates."""
import importlib.util
import os

import psycopg2

from odoo.tests import common, tagged
from odoo.tools import mute_logger

_MIGRATION = os.path.join(os.path.dirname(__file__), "..", "migrations", "1.1", "pre-dmarc-dedupe.py")


@tagged("post_install", "-at_install")
class TestDmarcUnique(common.TransactionCase):
    def setUp(self):
        super().setUp()
        self.Report = self.env["hams_base.dmarc.report"]
        self.vals = {"org_name": "uniq.example", "report_id": "r-1"}

    # Tests [@ANCHOR: hams_base:dmarc_one_report_per_org_and_id]
    # [@ANCHOR: test_a_second_report_with_the_same_org_and_id_is_refused]
    def test_a_second_report_with_the_same_org_and_id_is_refused(self):
        self.Report.create(self.vals)
        with mute_logger("odoo.sql_db"), self.assertRaises(psycopg2.IntegrityError):
            with self.env.cr.savepoint():
                self.Report.create(self.vals)
        self.Report.create({"org_name": "other.example", "report_id": "r-1"})
        self.Report.create({"org_name": "uniq.example", "report_id": False})
        self.Report.create({"org_name": "uniq.example", "report_id": False})

    # Tests [@ANCHOR: hams_base:migration_dmarc_report_dedupe]
    def test_migration_keeps_only_the_oldest_report(self):
        spec = importlib.util.spec_from_file_location("dmarc_report_dedupe", _MIGRATION)
        module = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(module)
        cr = self.env.cr
        cr.execute("ALTER TABLE hams_base_dmarc_report DROP CONSTRAINT hams_base_dmarc_report_one_report_per_org_and_id")
        first = self.Report.create(self.vals)
        self.Report.flush_model()
        cr.execute("INSERT INTO hams_base_dmarc_report (org_name, report_id) VALUES (%s, %s)", ("uniq.example", "r-1"))
        module.migrate(cr, "1.0")
        cr.execute("SELECT id FROM hams_base_dmarc_report WHERE org_name = 'uniq.example' AND report_id = 'r-1'")
        self.assertEqual([r[0] for r in cr.fetchall()], [first.id])
