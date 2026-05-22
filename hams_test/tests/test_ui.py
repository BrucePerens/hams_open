# -*- coding: utf-8 -*-
from odoo.tests import tagged
from odoo.addons.hams_test.common import HamsHttpCase

@tagged('-at_install', 'post_install')
class TestNoisyTableUI(HamsHttpCase):
    def test_01_tour(self):
        # trigger: .o_list_button_add
        # Tests [@ANCHOR: UX_NOISY_TABLE_MANAGEMENT]
        # [@ANCHOR: test_noisy_table_tour]
        # Verified by [@ANCHOR: test_noisy_table_tour]

        # Bypass fragile root menu navigation by jumping directly to the action endpoint
        # Enforcing ADR-0081 Section 8: Explicitly set ?debug=1 to prevent Owl dev mode crashes
        self.start_tour("/odoo?debug=1&action=hams_test.action_noisy_table", 'test_real_transaction_tour', login="admin")
