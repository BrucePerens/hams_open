# Copyright © Bruce Perens K6BP. All Rights Reserved. This software is proprietary and confidential.
from odoo.tests import tagged
from odoo.addons.zero_sudo.tests.common import HamsTransactionCase


@tagged("post_install", "-at_install")
class TestReportReporterIndex(HamsTransactionCase):
    """The report routes rate-limit and de-duplicate by the internal reporter; they must not scan the table."""

    def _single_column_index(self, table, column):
        self.env.cr.execute(
            "SELECT 1 FROM pg_indexes WHERE tablename = %s AND indexdef LIKE %s",
            (table, f"%({column})%"),
        )
        return bool(self.env.cr.fetchone())

    def test_reports_are_indexed_by_reporter(self):
        self.assertTrue(self._single_column_index("content_violation_report", "reported_by_user_id"))
