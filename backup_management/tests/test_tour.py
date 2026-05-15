# -*- coding: utf-8 -*-
from odoo.tests import HttpCase, tagged

@tagged('post_install', '-at_install')
class TestBackupTour(HttpCase):
    def test_backup_dashboard_tour(self):
        # Tests [@ANCHOR: test_tour_execution]
        self.start_tour("/web", 'backup_dashboard_tour', login="admin")
