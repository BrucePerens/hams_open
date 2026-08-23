# This software is distributed under the terms of the Affero General Public License (AGPL-3).

# -*- coding: utf-8 -*-
from odoo.tests.common import tagged
from odoo.addons.zero_sudo.tests.common import HamsHttpCase


@tagged("post_install", "-at_install")
class TestUITours(HamsHttpCase):
    def test_pager_duty_incident_tour(self):
        self.start_tour("/odoo?debug=1", "pager_duty_incident_tour", login="admin")

    def test_pager_check_tour(self):
        # pager_check_views.xml's form has ~15 dynamic invisible= fields
        # keyed off check_type -- a real ADR-0076 "Complex State Machine"
        # that was flagged (via a live TODO comment) as missing its
        # mandated tour. See docs/adrs/0076_ui_tour_mandate_and_bypass_governance.md.
        self.start_tour("/odoo?debug=1", "pager_check_tour", login="admin")
