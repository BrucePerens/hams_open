# -*- coding: utf-8 -*-
# Copyright © Bruce Perens K6BP.
# SPDX-License-Identifier: AGPL-3.0-or-later
from odoo.tests.common import tagged
from odoo.addons.zero_sudo.tests.common import HamsHttpCase


@tagged("post_install", "-at_install")
class TestComplianceIndexSignedIn(HamsHttpCase):
    def test_a_signed_in_member_sees_the_compliance_directory(self):
        # Found live 2026-10-05: /compliance answered 403 to a signed-in member, because only
        # base.group_public could read compliance.document and a portal or internal user is not in it.
        # Tests [@ANCHOR: COMM_compliance_index_route]
        user = self.env["res.users"].create({
            "name": "Compliance Reader", "login": "compliance_reader", "email": "compliance_reader@example.com",
            "group_ids": [(6, 0, [self.env.ref("base.group_portal").id])],
        })
        user.password = "compliance_reader"
        self.authenticate("compliance_reader", "compliance_reader")
        response = self.url_open("/compliance")
        self.assertEqual(response.status_code, 200)
        self.assertIn("Privacy", response.text)
