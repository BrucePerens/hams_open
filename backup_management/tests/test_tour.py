# -*- coding: utf-8 -*-
from odoo.tests import tagged
from odoo.addons.hams_test.tests.real_transaction import HamsHttpCase


@tagged("post_install", "-at_install")
class TestBackupTour(HamsHttpCase):
    def test_backup_dashboard_tour(self):
        # Tests [@ANCHOR: test_tour_execution]
        self.start_tour("/odoo", "backup_dashboard_tour", login="admin")
