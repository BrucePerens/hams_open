# -*- coding: utf-8 -*-
import time
from odoo.tests.common import tagged
from odoo.addons.zero_sudo.tests.real_transaction import RealTransactionCase


@tagged("post_install", "-at_install")
class TestModeration(RealTransactionCase):

    def setUp(self):
        super(TestModeration, self).setUp()

        # 1. Create a misbehaving user
        self.bad_user = self.env["res.users"].create(
            {
                "name": "Spammer",
                "login": f"spammer_{self.id()}",
                "email": "spam@example.com",
                "website_slug": f"spammer_{self.id()}",
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

        # 2. Give them some published content
        self.spam_page = self.env["website.page"].create(
            {
                "url": f"/{self.bad_user.website_slug}/home",
                "name": "Spam Home",
                "type": "qweb",
                "arch": '<t name="Spam Home" t-name="user_websites.spam_home"><div>Spam</div></t>',
                "owner_user_id": self.bad_user.id,
                "is_published": True,
                "website_published": True,
            }
        )

        blog = self.env["blog.blog"].create({"name": "Community Blog"})

        self.spam_post = self.env["blog.post"].create(
            {
                "name": "Spam Post",
                "blog_id": blog.id,
                "owner_user_id": self.bad_user.id,
                "is_published": True,
            }
        )
        self.env.cr.commit()

    def test_01_three_strikes_suspension(self):
        # [@ANCHOR: test_moderation_suspension]
        # Tests [@ANCHOR: action_take_action_and_strike]
        """
        Verify that hitting 3 strikes automatically suspends the user
        and unpublishes all their content.
        """
        # Ensure starting state is clean
        self.assertEqual(self.bad_user.violation_strike_count, 0)
        self.assertFalse(self.bad_user.is_suspended_from_websites)
        self.assertTrue(self.spam_page.is_published)
        self.assertTrue(self.spam_post.is_published)

        # Admin creates and processes 3 reports
        for i in range(3):
            report = self.env["content.violation.report"].create(
                {
                    "target_url": f"/test/spam/{i}",
                    "description": f"Spam instance {i}",
                    "content_owner_id": self.bad_user.id,
                }
            )
            report.action_take_action_and_strike()

        # Verify final state
        self.assertEqual(self.bad_user.violation_strike_count, 3, "[!] DIAGNOSTIC FOR AI: Strike count mismatch. Ensure action_take_action_and_strike increments the count.")
        self.assertTrue(
            self.bad_user.is_suspended_from_websites,
            "User should be suspended after 3 strikes. [!] DIAGNOSTIC FOR AI: is_suspended_from_websites flag not set. Check action_suspend_user_websites trigger logic.",
        )
        # Wait for the background thread to unpublish the content
        self.env.cr.commit()
        for _ in range(20):
            self.env.cr.commit()
            self.env.invalidate_all()
            if not self.spam_page.is_published and not self.spam_post.is_published:
                time.sleep(0.5) # audit-ignore-sleep
                break
            time.sleep(0.5) # audit-ignore-sleep

        # Verify Content was unpublished
        self.assertFalse(
            self.spam_page.is_published, "Page should be forcefully unpublished. [!] DIAGNOSTIC FOR AI: spam_page.is_published is still True. Ensure action_suspend_user_websites unpublishes pages."
        )
        self.assertFalse(
            self.spam_post.is_published, "Blog post should be forcefully unpublished. [!] DIAGNOSTIC FOR AI: spam_post.is_published is still True. Ensure action_suspend_user_websites unpublishes blog posts."
        )

    def test_02_pardon_functionality(self):
        """Verify the pardon action resets strikes and lifts suspension."""
        self.bad_user.violation_strike_count = 3
        self.bad_user.action_suspend_user_websites()
        self.env.cr.commit()
        for _ in range(20):
            self.env.cr.commit()
            self.env.invalidate_all()
            if not self.spam_page.is_published and not self.spam_post.is_published:
                # Wait for the background transaction to fully commit
                time.sleep(0.5) # audit-ignore-sleep
                break
            time.sleep(0.5) # audit-ignore-sleep

        self.assertTrue(self.bad_user.is_suspended_from_websites, "[!] DIAGNOSTIC FOR AI: Failed to suspend user before pardon test.")

        # Admin pardons user
        self.bad_user.action_pardon_user_websites()

        self.assertEqual(self.bad_user.violation_strike_count, 0, "[!] DIAGNOSTIC FOR AI: Pardon failed to reset strike count.")
        self.assertFalse(self.bad_user.is_suspended_from_websites, "[!] DIAGNOSTIC FOR AI: Pardon failed to lift suspension flag.")
        # Note: We intentionally do NOT automatically republish content during a pardon.
        # The user must do that manually to ensure they reviewed it.
        self.assertFalse(self.spam_page.is_published)

    def test_03_suspension_public_access_leak(self):
        """
        Verify that action_suspend_user_websites strictly revokes public access
        and does not inadvertently set or leave public access grants during unpublication.
        """
        # Ensure page is public
        self.authenticate(None, None)
        self.env.cr.commit()
        res = self.url_open(f"/{self.bad_user.website_slug}/home")
        self.assertEqual(res.status_code, 200)

        # Suspend user
        self.bad_user.violation_strike_count = 3
        self.bad_user.action_suspend_user_websites()
        self.env.cr.commit()
        for _ in range(20):
            self.env.cr.commit()
            self.env.invalidate_all()
            if not self.spam_page.is_published and not self.spam_post.is_published:
                # Wait for the background transaction to fully commit
                time.sleep(0.5) # audit-ignore-sleep
                break
            time.sleep(0.5) # audit-ignore-sleep

        # Attempt public access again
        self.env.cr.commit()
        res_after = self.url_open(f"/{self.bad_user.website_slug}/home")
        self.assertEqual(
            res_after.status_code,
            404,
            "Suspended pages must return 404 for public guests to prevent access leaks.",
        )

    def test_04_group_moderation_cascading_strikes(self):
        """
        Verify that applying a strike to a group violation report correctly iterates
        and applies strikes to all member_ids of that group.
        """
        user_2 = self.env["res.users"].create(
            {
                "name": "Spammer 2",
                "login": f"spammer2_{self.id()}",
                "email": "spam2@example.com",
                "website_slug": f"spammer2_{self.id()}",
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

        bad_group = self.env["user.websites.group"].create(
            {
                "name": "Bad Group",
                "website_slug": "bad-group",
                "member_ids": [(4, self.bad_user.id), (4, user_2.id)],
            }
        )

        # Start at 0
        self.assertEqual(self.bad_user.violation_strike_count, 0)
        self.assertEqual(user_2.violation_strike_count, 0)

        report = self.env["content.violation.report"].create(
            {
                "target_url": "/bad-group/home",
                "description": "Group is spamming",
                "content_group_id": bad_group.id,
            }
        )

        # Strike the group
        report.action_take_action_and_strike()

        self.assertEqual(
            self.bad_user.violation_strike_count,
            1,
            "Strike should cascade to member 1.",
        )
        self.assertEqual(
            user_2.violation_strike_count, 1, "Strike should cascade to member 2."
        )

        # Apply 2 more strikes to trigger the automated 3-strike suspension rule
        for i in range(2):
            r = self.env["content.violation.report"].create(
                {
                    "target_url": f"/bad-group/page{i}",
                    "description": "More spam",
                    "content_group_id": bad_group.id,
                }
            )
            r.action_take_action_and_strike()

        self.assertTrue(
            self.bad_user.is_suspended_from_websites,
            "Member 1 should be suspended after 3 strikes.",
        )
        self.assertTrue(
            user_2.is_suspended_from_websites,
            "Member 2 should be suspended after 3 strikes.",
        )

        for _ in range(20):
            self.env.cr.commit()
            self.env.invalidate_all()
            if not self.spam_page.is_published:
                # Wait for the background transaction to fully commit
                time.sleep(0.5) # audit-ignore-sleep
                break
            time.sleep(0.5) # audit-ignore-sleep

    def test_05_concurrent_strike_locking(self):
        """
        Verify that action_take_action_and_strike issues a FOR NO KEY UPDATE lock
        to prevent 'Lost Update' race conditions during concurrent moderation.
        """
        # 1. Test Individual User Lock
        report = self.env["content.violation.report"].create(
            {
                "target_url": "/test/lock",
                "description": "Lock test",
                "content_owner_id": self.bad_user.id,
            }
        )

        mock_execute = self.safe_patch_object(
            self.env.cr, "execute", wraps=self.env.cr.execute
        )
        report.action_take_action_and_strike()

        # Assert the lock query was injected
        lock_query = "SELECT id FROM res_users WHERE id = %s FOR NO KEY UPDATE"
        mock_execute.assert_any_call(lock_query, (self.bad_user.id,))

        # 2. Test Group Member Lock
        group_report = self.env["content.violation.report"].create(
            {
                "target_url": "/test/group_lock",
                "description": "Group Lock test",
                "content_group_id": self.env["user.websites.group"]
                .create(
                    {
                        "name": "Lock Group",
                        "website_slug": "lock-group",
                        "member_ids": [(4, self.bad_user.id)],
                    }
                )
                .id,
            }
        )

        mock_execute = self.safe_patch_object(
            self.env.cr, "execute", wraps=self.env.cr.execute
        )
        group_report.action_take_action_and_strike()

        lock_query_group = (
            "SELECT id FROM res_users WHERE id IN %s FOR NO KEY UPDATE"
        )
        mock_execute.assert_any_call(
            lock_query_group, (tuple(group_report.content_group_id.member_ids.ids),)
        )
