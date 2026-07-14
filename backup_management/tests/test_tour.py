# -*- coding: utf-8 -*-
# Copyright © Bruce Perens K6BP. All Rights Reserved.
# This software is released under the AGPL-3.0 License.
from odoo.tests import tagged
from odoo.addons.zero_sudo.tests.common import HamsHttpCase


@tagged("post_install", "-at_install")
class TestBackupTour(HamsHttpCase):
    def test_backup_dashboard_tour(self):
        # Tests [@ANCHOR: backup_management:backup_dashboard_tour]
        self.start_tour("/odoo?debug=1", "backup_dashboard_tour", login="admin")
