# Copyright © Bruce Perens K6BP.
# SPDX-License-Identifier: AGPL-3.0-or-later
from odoo.addons.zero_sudo.tests.common import HamsTransactionCase
from odoo.tests.common import tagged


@tagged("post_install", "-at_install")
class TestSupportMenuItem(HamsTransactionCase):
    """The "Support" website menu item for /my/tickets."""

    # [@ANCHOR: test_hams_helpdesk_support_menu_item]
    # Tests [@ANCHOR: hams_helpdesk:member_menu_item]
    def test_support_item_is_member_only_and_points_at_tickets(self):
        item = self.env.ref("hams_helpdesk.menu_hams_support")
        self.assertEqual(item.url, "/my/tickets")
        groups = item.group_ids
        self.assertIn(self.env.ref("base.group_portal"), groups)
        self.assertIn(self.env.ref("base.group_user"), groups)
        public = self.env.ref("base.public_user")
        visible = (
            self.env["website.menu"]
            .with_user(public)
            .search([("id", "=", item.id)])
        )
        self.assertFalse(visible)
