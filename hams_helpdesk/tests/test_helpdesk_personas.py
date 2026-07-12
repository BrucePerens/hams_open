# This software is distributed under the terms of the Affero General Public License (AGPL-3).

# -*- coding: utf-8 -*-
from odoo.tests.common import tagged
from odoo.addons.zero_sudo.tests.common import HamsHttpCase


@tagged("post_install", "-at_install", "ui", "standard")
class TestHelpdeskPersonasUITours(HamsHttpCase):

    def setUp(self):
        super().setUp()
        self.crm_user = self.env["res.users"].create(
            {
                "name": "CRM User",
                "login": "crm_user_tour",
                "password": "password",
                "email": "crm_tour@example.com",
                "group_ids": [
                    (
                        6,
                        0,
                        [
                            self.env.ref("base.group_portal").id,
                        ],
                    )
                ],
            }
        )
        self.club_user = self.env["res.users"].create(
            {
                "name": "Club Rep User",
                "login": "club_user_tour",
                "password": "password",
                "email": "club_tour@example.com",
                "group_ids": [
                    (
                        6,
                        0,
                        [
                            self.env.ref("base.group_portal").id,
                            # Add club group if present, else just portal
                        ],
                    )
                ],
            }
        )

    def test_01_crm_user_tour(self):
        self.start_tour(
            "/my/tickets?debug=1", "helpdesk_portal_tour", login="crm_user_tour"
        )

    def test_02_club_user_tour(self):
        self.start_tour(
            "/my/tickets?debug=1", "helpdesk_portal_tour", login="club_user_tour"
        )
