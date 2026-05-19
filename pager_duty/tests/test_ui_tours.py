# -*- coding: utf-8 -*-
from odoo.tests.common import tagged
from odoo.addons.hams_test.tests.real_transaction import HamsHttpCase

@tagged("post_install", "-at_install")
class TestUITours(HamsHttpCase):
    def test_pager_duty_incident_tour(self):
        # NOTE: Not using IN_JULES_VM bypass here as per review feedback.
        self.start_tour("/odoo?debug=1", "pager_duty_incident_tour", login="admin")
