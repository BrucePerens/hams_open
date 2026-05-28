# -*- coding: utf-8 -*-
from odoo.tests.common import tagged
from odoo.addons.zero_sudo.tests.common import HamsTransactionCase
from odoo.exceptions import AccessError


@tagged("post_install", "-at_install")
class TestManualAccessRights(HamsTransactionCase):

    def setUp(self):
        super(TestManualAccessRights, self).setUp()

        # 1. Setup Personas
        self.admin_user = self.env["res.users"].create(
            {
                "name": "Manual Admin",
                "login": "manual_admin",
                "email": "admin@manual.com",
                "group_ids": [
                    (
                        6,
                        0,
                        [
                            self.env.ref("base.group_portal").id,
                            self.env.ref("manual_library.group_manual_manager").id,
                        ],
                    )
                ],
            }
        )

        self.internal_user = self.env["res.users"].create(
            {
                "name": "Standard Internal",
                "login": "internal_user",
                "email": "internal@manual.com",
                "group_ids": [(6, 0, [self.env.ref("base.group_portal").id])],
            }
        )

        self.other_internal_user = self.env["res.users"].create(
            {
                "name": "Other Internal",
                "login": "other_internal_user",
                "email": "other@manual.com",
                "group_ids": [(6, 0, [self.env.ref("base.group_portal").id])],
            }
        )

        self.public_user = self.env.ref("base.public_user")

        # 2. Setup Contextual Articles
        self.published_article = self.env["knowledge.article"].create(
            {
                "name": "Public Published Guide",
                "is_published": True,
                "internal_permission": "read",
            }
        )

        self.unpublished_workspace_article = self.env["knowledge.article"].create(
            {
                "name": "Internal Draft",
                "is_published": False,
                "internal_permission": "read",
            }
        )

        self.private_article = self.env["knowledge.article"].create(
            {
                "name": "Top Secret Admin Notes",
                "is_published": False,
                "internal_permission": "none",
            }
        )

    def test_01_admin_full_crud(self):
        """Admin should have full Create, Read, Update, Delete access to all articles."""
        # Read
        self.assertTrue(self.private_article.with_user(self.admin_user).name)

        # Write
        self.private_article.with_user(self.admin_user).write(
            {"name": "Updated Secret"}
        )
        self.assertEqual(self.private_article.name, "Updated Secret")

        # Create
        new_article = (
            self.env["knowledge.article"]
            .with_user(self.admin_user)
            .create({"name": "Admin Created"})
        )
        self.assertTrue(new_article.id)

        # Delete
        new_article.with_user(self.admin_user).unlink()
        self.assertFalse(new_article.exists())

    def test_02_internal_user_contextual_read(self):
        """Internal users can read workspace articles, but not private ones with 'none' permission."""
        # Success: Read Workspace Article
        try:
            name = self.unpublished_workspace_article.with_user(self.internal_user).name
            self.assertEqual(name, "Internal Draft")
        except AccessError:
            self.fail("Internal user must be able to read workspace articles.")

        # Failure: Read Private Article
        with self.assertRaises(
            AccessError, msg="Internal user should be blocked from private articles."
        ):
            _ = self.private_article.with_user(self.internal_user).name

    def test_03_internal_user_mutation_blocked(self):
        """Internal users are strictly blocked from Create, Write, or Unlink."""
        with self.assertRaises(AccessError):
            self.unpublished_workspace_article.with_user(self.internal_user).write(
                {"name": "Hacked"}
            )
            self.env.flush_all()

        with self.assertRaises(AccessError):
            self.env["knowledge.article"].with_user(self.internal_user).create(
                {"name": "Rogue Article"}
            )
            self.env.flush_all()

        with self.assertRaises(AccessError):
            self.unpublished_workspace_article.with_user(self.internal_user).unlink()

    def test_04_public_guest_contextual_read(self):
        """Guests can only read published articles."""
        # Success: Read Published
        try:
            name = self.published_article.with_user(self.public_user).name
            self.assertEqual(name, "Public Published Guide")
        except AccessError:
            self.fail("Public user must be able to read published articles.")

        # Failure: Read Unpublished
        with self.assertRaises(
            AccessError, msg="Public user must be blocked from unpublished articles."
        ):
            _ = self.unpublished_workspace_article.with_user(self.public_user).name

    def test_05_public_guest_mutation_blocked(self):
        """Guests cannot mutate any data."""
        with self.assertRaises(AccessError):
            self.published_article.with_user(self.public_user).write(
                {"name": "Vandalized"}
            )
            self.env.flush_all()

        with self.assertRaises(AccessError):
            self.env["knowledge.article"].with_user(self.public_user).create(
                {"name": "Spam Article"}
            )
            self.env.flush_all()

    def test_06_private_creator_access(self):
        """Users who create a private article should implicitly have read access to it."""
        # Internal user creates an article and sets it to private
        my_private_article = (
            self.env["knowledge.article"]
            .with_user(self.admin_user)
            .create({"name": "My Personal Notes", "internal_permission": "none"})
        )
        # Force the create_uid to be our internal user to simulate them creating it
        # (since standard users lack create permission globally in this module)
        self.env.cr.execute(
            "UPDATE knowledge_article SET create_uid = %s WHERE id = %s",
            (self.internal_user.id, my_private_article.id),
        )
        my_private_article.invalidate_recordset()

        # The creator should be able to read it
        try:
            name = my_private_article.with_user(self.internal_user).name
            self.assertEqual(name, "My Personal Notes")
        except AccessError:
            self.fail("Creator must be able to read their own private articles.")

        # But another internal user should NOT
        with self.assertRaises(AccessError):
            _ = my_private_article.with_user(self.other_internal_user).name

    def test_08_owner_unpublished_visibility(self):
        """Owners must be able to see their own articles even if they are NOT published."""
        unpublished_owned = self.env["knowledge.article"].create({
            "name": "My Secret Draft",
            "is_published": False,
        })
        self.env.cr.execute(
            "UPDATE knowledge_article SET create_uid = %s WHERE id = %s",
            (self.internal_user.id, unpublished_owned.id),
        )
        unpublished_owned.invalidate_recordset()

        # Should be readable by owner
        try:
            name = unpublished_owned.with_user(self.internal_user).name
            self.assertEqual(name, "My Secret Draft")
        except AccessError:
            self.fail("Owner must be able to see their own unpublished articles.")

    def test_07_shared_article_access(self):
        """Users explicitly added to member_ids should be able to access private shared articles."""
        shared_article = self.env["knowledge.article"].create(
            {
                "name": "Shared Project Data",
                "internal_permission": "none",
                "member_ids": [(4, self.internal_user.id)],
            }
        )

        # Explicit member should be able to read
        try:
            name = shared_article.with_user(self.internal_user).name
            self.assertEqual(name, "Shared Project Data")
        except AccessError:
            self.fail(
                "Members added to member_ids must be able to access the shared article."
            )

        # Non-member should be blocked
        with self.assertRaises(AccessError):
            _ = shared_article.with_user(self.other_internal_user).name
