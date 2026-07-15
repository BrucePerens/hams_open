# This software is distributed under the terms of the Affero General Public License (AGPL-3).

# -*- coding: utf-8 -*-
from odoo.tests import tagged
from odoo.addons.zero_sudo.tests.common import HamsHttpCase


@tagged("post_install", "-at_install")
class TestTours(HamsHttpCase):

    def setUp(self):
        super().setUp()
        self.env.ref("base.user_admin").lang = "en_US"

    def test_01_db_bloat_tour(self):
        # [@ANCHOR: COMM_test_db_bloat_tour]
        self.start_tour("/odoo?debug=1", "db_management_bloat_tour", login="admin")

    def test_02_db_slow_query_tour(self):
        # [@ANCHOR: COMM_test_db_slow_query_tour]
        self.start_tour("/odoo?debug=1", "db_management_slow_query_tour", login="admin")
