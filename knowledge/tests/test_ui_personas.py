# Copyright © Bruce Perens K6BP.
# SPDX-License-Identifier: AGPL-3.0-or-later

# -*- coding: utf-8 -*-
from odoo.tests.common import tagged
from odoo.addons.zero_sudo.tests.common import HamsHttpCase


@tagged("post_install", "-at_install", "ui", "standard")
class TestManualPersonasUITours(HamsHttpCase):

    def setUp(self):
        super().setUp()
        # The public-user tour needs open sign-up: with ham_init installed the manual is hidden from a
        # visitor who is not signed in while the site is invite only.
        self.env["ir.config_parameter"].set_param("auth_signup.invitation_scope", "b2c")
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
