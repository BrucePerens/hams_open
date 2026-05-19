# -*- coding: utf-8 -*-
from odoo.tests.common import tagged
from odoo.addons.hams_test.tests.real_transaction import HamsTransactionCase
from odoo.exceptions import AccessError


@tagged("post_install", "-at_install")
class TestSecurityEdgeCases(HamsTransactionCase):

    def setUp(self):
        super(TestSecurityEdgeCases, self).setUp()

        self.user_owner = self.env["res.users"].create(
            {
                "name": "Owner User",
                "login": "owner",
                "website_slug": "owner",
                "group_ids": [
                    (
                        6,
                        0,
                        [
                            self.env.ref("base.group_portal").id,
                            self.env.ref("user_websites.group_user_websites_user").id,
                        ],
                    )
                ],
            }
        )

        self.user_intruder = self.env["res.users"].create(
            {
                "name": "Intruder User",
                "login": "intruder",
                "website_slug": "intruder",
                "group_ids": [
                    (
                        6,
                        0,
                        [
                            self.env.ref("base.group_portal").id,
                            self.env.ref("user_websites.group_user_websites_user").id,
                        ],
                    )
                ],
            }
        )

        self.test_group = self.env["user.websites.group"].create(
            {
                "name": "Edge Case Group",
                "website_slug": "edge-case-group",
                "member_ids": [(4, self.user_owner.id)],
            }
        )

        self.personal_page = self.env["website.page"].create(
            {
                "url": "/owner/page",
                "name": "Owner Page",
                "type": "qweb",
                "owner_user_id": self.user_owner.id,
            }
        )

        self.group_page = self.env["website.page"].create(
            {
                "url": "/edge-case-group/page",
                "name": "Group Page",
                "type": "qweb",
                "user_websites_group_id": self.test_group.id,
            }
        )

    def test_01_intruder_cannot_write_personal_page(self):
        """Intruder tries to edit Owner's personal page."""
        with self.assertRaises(AccessError):
            self.personal_page.with_user(self.user_intruder).write({"name": "Hacked"})
            self.env.flush_all()

    def test_02_intruder_cannot_unlink_personal_page(self):
        """Intruder tries to delete Owner's personal page."""
        with self.assertRaises(AccessError):
            self.personal_page.with_user(self.user_intruder).unlink()

    def test_03_intruder_cannot_write_group_page(self):
        """Intruder tries to edit a Group page they don't belong to."""
        with self.assertRaises(AccessError):
            self.group_page.with_user(self.user_intruder).write(
                {"name": "Hacked Group"}
            )
            self.env.flush_all()

    def test_04_owner_can_write_personal_and_group_page(self):
        """Ensure the true owner/member CAN write to their own resources."""
        # Should not raise exception
        self.personal_page.with_user(self.user_owner).write(
            {"name": "Updated Owner Page"}
        )
        self.group_page.with_user(self.user_owner).write({"name": "Updated Group Page"})

        self.assertEqual(self.personal_page.name, "Updated Owner Page")
        self.assertEqual(self.group_page.name, "Updated Group Page")

    def test_05_intruder_cannot_read_private_group_settings(self):
        """Ensure intruder cannot modify the private group record itself."""
        with self.assertRaises(AccessError):
            self.test_group.with_user(self.user_intruder).write(
                {"name": "Hacked Group Name"}
            )
            self.env.flush_all()

    def test_06_intruder_cannot_unlink_group_page(self):
        """Ensure intruder cannot delete a Group page they do not have membership in."""
        with self.assertRaises(
            AccessError,
            msg="Record rules must block non-members from unlinking group pages.",
        ):
            self.group_page.with_user(self.user_intruder).unlink()
