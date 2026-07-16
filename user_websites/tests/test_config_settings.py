# -*- coding: utf-8 -*-
# Copyright © Bruce Perens K6BP.
# SPDX-License-Identifier: AGPL-3.0-or-later
from odoo.tests.common import tagged
from odoo.addons.zero_sudo.tests.common import HamsTransactionCase


@tagged("post_install", "-at_install")
class TestConfigSettings(HamsTransactionCase):

    def setUp(self):
        super(TestConfigSettings, self).setUp()
        self.admin_group = self.env.ref(
            "user_websites.group_user_websites_administrator"
        )

        self.user_admin_test = self.env["res.users"].create(
            {
                "name": "Settings Admin Test",
                "login": "settingsadmin",
                "email": "settingsadmin@example.com",
                "website_slug": "settingsadmin",
                "group_ids": [(6, 0, [])],
            }
        )

    def test_01_settings_sync_with_group(self):
        """
        Ensure that setting values in the ResConfigSettings TransientModel properly
        updates the underlying res.groups mapping, and vice-versa.
        """
        # Step 1: Add user via settings
        settings = self.env["res.config.settings"].create(
            {"user_websites_administrators_ids": [(4, self.user_admin_test.id)]}
        )
        settings.set_values()

        # Verify user is now in the security group
        self.assertIn(
            self.user_admin_test,
            self.admin_group.user_ids,
            "User should be added to the Administrator group via settings.",
        )

        # Step 2: Read values back via settings
        new_settings = self.env["res.config.settings"].create({})
        retrieved_values = new_settings.get_values()

        self.assertIn(
            self.user_admin_test.id,
            retrieved_values.get("user_websites_administrators_ids", [])[0][2],
            "get_values should accurately pull users from the Administrator group.",
        )

        # Step 3: Remove user via settings
        clear_settings = self.env["res.config.settings"].create(
            {"user_websites_administrators_ids": [(3, self.user_admin_test.id)]}
        )
        clear_settings.set_values()

        self.assertNotIn(
            self.user_admin_test,
            self.admin_group.user_ids,
            "User should be removed from the Administrator group via settings.",
        )
