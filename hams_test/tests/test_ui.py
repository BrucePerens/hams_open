# -*- coding: utf-8 -*-
from odoo.tests import tagged
from odoo.addons.hams_test.tests.real_transaction import HamsHttpCase

@tagged('-at_install', 'post_install')
class TestNoisyTableUI(HamsHttpCase):
    def test_01_tour(self):
        # trigger: .o_list_button_add
        # Tests [@ANCHOR: UX_NOISY_TABLE_MANAGEMENT]
        # [@ANCHOR: test_noisy_table_tour]
        # Verified by [@ANCHOR: test_noisy_table_tour]

        # Bypass fragile root menu navigation by jumping directly to the action endpoint
        self.start_tour("/odoo?action=hams_test.action_noisy_table", 'test_real_transaction_tour', login="admin")
