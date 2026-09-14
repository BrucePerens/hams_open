# This software is distributed under the terms of the Affero General Public License (AGPL-3).

# -*- coding: utf-8 -*-

from odoo.tests import tagged
from odoo.exceptions import AccessError
from odoo.addons.zero_sudo.tests.common import HamsTransactionCase


@tagged("post_install", "-at_install")
class TestAccessRights(HamsTransactionCase):
    def setUp(self):
        super(TestAccessRights, self).setUp()
        self.user_websites_admin_group = self.env.ref(
            "user_websites.group_user_websites_administrator"
        )

        self.test_user_1 = self.env["res.users"].create(
            {
                "name": "Test User 1",
                "login": "testuser1",
                "email": "testuser1@example.com",
                "website_slug": "testuser1",
                "group_ids": [(6, 0, [self.env.ref("base.group_portal").id])],
            }
        )

        self.websites_admin_user = self.env["res.users"].create(
            {
                "name": "Websites Admin",
                "login": "websitesadmin",
                "email": "websitesadmin@example.com",
                "website_slug": "websitesadmin",
                "group_ids": [
                    (
                        6,
                        0,
                        [
                            self.user_websites_admin_group.id,
                        ],
                    )
                ],
                "password": "websitesadmin",
            }
        )

        self.regular_user = self.env["res.users"].create(
            {
                "name": "Regular User",
                "login": "reguser",
                "email": "reguser@example.com",
                "website_slug": "reguser",
                "group_ids": [(6, 0, [self.env.ref("base.group_portal").id])],
                "password": "reguser",
            }
        )

    def test_01_regular_user_cannot_access_settings(self):
        with self.assertRaises(AccessError):
            self.env["res.config.settings"].with_user(self.regular_user).create(
                {}
            )
            self.env.flush_all()

    def test_02_admin_can_access_settings_and_see_field(self):
        try:
            self.env["res.config.settings"].with_user(
                self.env.ref("base.user_admin")
            ).check_access("write")
            access = True
        except AccessError:
            access = False

        self.assertTrue(access, "Admin should have write access to settings")

    def test_03_websites_admin_cannot_access_settings(self):
        """
        night_shift_todo.md "saving ANY Settings page can crash with an
        AccessError", full design writeup: this test used to assert the
        OPPOSITE of what it asserts now -- that group_user_websites_
        administrator (a content-moderation-tier role; it does not imply
        base.group_system) DID have write access to the whole shared
        res.config.settings model, treating a real privilege-escalation
        hole as a feature. `ir.model.access.csv` grants are per (model,
        group), never per field, so that access was never actually scoped
        to this module's own fields -- it covered every field any installed
        module merges onto this one TransientModel (see test_config_
        settings.py's test_04 for the concrete cross-module proof: this
        role could read/write distributed_redis_cache's redis_password and
        cloudflare's cloudflare_api_token). The access-csv row that granted
        this (`access_res_config_settings_admin`) has been deleted; this
        role now has no access to res.config.settings at all, matching
        every other non-base.group_system group in both repos.
        """
        with self.assertRaises(AccessError):
            self.env["res.config.settings"].with_user(
                self.websites_admin_user
            ).check_access("write")

    def test_04_public_cannot_access_settings(self):
        """
        Verify that a guest (public user) cannot access configuration settings.
        """
        public_user = self.env.ref("base.public_user")

        with self.assertRaises(AccessError):
            self.env["res.config.settings"].with_user(public_user).check_access("write")

    def test_05_owner_cannot_read_own_violation_reports(self):
        """
        Verify that a content owner cannot see reports filed against their own content,
        protecting the identity of the complainant.
        """
        # 1. Simulate a public user or admin creating a report against test_user_1's content
        report = self.env["content.violation.report"].create(
            {
                "target_url": f"/{self.test_user_1.website_slug}/page",
                "description": "Inappropriate content",
                "reported_by_email": "concerned_citizen@example.com",
                "content_owner_id": self.test_user_1.id,
            }
        )

        # 2. Test User 1 attempts to search for the report
        visible_reports = (
            self.env["content.violation.report"]
            .with_user(self.test_user_1)
            .search([("id", "=", report.id)], limit=1)
        )

        self.assertFalse(
            visible_reports,
            "The record rule should hide the report from the content owner during searches.",
        )

        # 3. Test User 1 attempts to read the report directly by ID
        with self.assertRaises(
            AccessError,
            msg="The content owner should be blocked from directly reading reports against their content.",
        ):
            report.with_user(self.test_user_1).read(
                ["description", "reported_by_email"]
            )
