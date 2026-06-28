# -*- coding: utf-8 -*-
from odoo.tests.common import tagged
from odoo.addons.zero_sudo.tests.common import HamsHttpCase


@tagged("post_install", "-at_install")
class TestUITours(HamsHttpCase):
    def test_pager_duty_incident_tour(self):
        self.start_tour("/odoo?debug=1", "pager_duty_incident_tour", login="admin")
