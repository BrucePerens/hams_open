# This software is distributed under the terms of the Affero General Public License (AGPL-3).

# -*- coding: utf-8 -*-
from odoo.tests.common import tagged
from odoo.addons.zero_sudo.tests.common import HamsHttpCase


@tagged("post_install", "-at_install", "ui", "standard")
class TestManualPersonasUITours(HamsHttpCase):

    def setUp(self):
        super().setUp()
        self.portal_user = self.env["res.users"].create(
            {
                "name": "Portal User",
                "login": "portal_user_tour",
                "password": "password",
                "email": "portal_tour@example.com",
                "group_ids": [(6, 0, [self.env.ref("base.group_portal").id])],
            }
        )

        self.article = self.env["knowledge.article"].create(
            {
                "name": "Persona Test Article",
                "body": "<p>Content for persona testing.</p>",
                "is_published": True,
            }
        )

    def test_01_public_user_tour(self):
        self.start_tour("/manual?debug=1", "manual_basic_browsing_tour")

    def test_02_portal_user_tour(self):
        self.start_tour(
            "/manual?debug=1", "manual_basic_browsing_tour", login="portal_user_tour"
        )

    def test_03_admin_user_tour(self):
        self.start_tour("/manual?debug=1", "manual_basic_browsing_tour", login="admin")
