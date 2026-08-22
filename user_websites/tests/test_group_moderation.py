# -*- coding: utf-8 -*-
# Copyright © Bruce Perens K6BP.
# SPDX-License-Identifier: AGPL-3.0-or-later
from odoo.tests.common import tagged
from odoo.addons.zero_sudo.tests.real_transaction import RealTransactionCase
import time


@tagged("post_install", "-at_install")
class TestGroupModeration(RealTransactionCase):

    def setUp(self):
        super(TestGroupModeration, self).setUp()

        self.group = self.env["user.websites.group"].create(
            {
                "name": f"Bad Group {self.id()}",
                "website_slug": f"bad_group_{self.id()}",
            }
        )

        self.group_page = self.env["website.page"].create(
            {
                "url": f"/{self.group.website_slug}/home",
                "name": "Group Home",
                "type": "qweb",
                "arch": '<t name="Group Home" t-name="user_websites.group_home"><div>Spam</div></t>',
                "user_websites_group_id": self.group.id,
                "is_published": True,
                "website_published": True,
            }
        )

        blog = self.env["blog.blog"].create({"name": "Community Blog"})

        self.group_post = self.env["blog.post"].create(
            {
                "name": "Group Spam Post",
                "blog_id": blog.id,
                "user_websites_group_id": self.group.id,
                "is_published": True,
            }
        )
        self.env.cr.commit()

    def test_01_group_suspension(self):
        # Suspend the group
        self.group.action_suspend_group_websites()
        self.env.cr.commit()

        # Verify suspension
        self.assertTrue(self.group.is_suspended_from_websites)

        # action_suspend_group_websites() offloads the actual unpublish
        # work to a real background thread (BACKGROUND_EXECUTOR) that
        # opens its own Registry/cursor -- a fixed sleep is inherently
        # racy against however long that takes under real system load
        # (including, on a cold thread, building a fresh registry), so
        # poll for the real committed state instead of guessing a
        # duration.
        deadline = time.time() + 30.0
        while time.time() < deadline:
            # This transaction (RealTransactionCase) runs under PostgreSQL
            # REPEATABLE READ -- invalidate_recordset() only clears Odoo's
            # own Python-side field cache, it does NOT advance the
            # underlying PG snapshot, so without a real commit() here this
            # loop would poll the exact same frozen-at-first-commit
            # snapshot 120 times and could never observe the background
            # thread's write no matter how long it waited. A plain
            # commit() (no pending writes of our own at this point) ends
            # the current snapshot and starts a fresh one able to see
            # other connections' committed changes. Root-caused via a
            # real background-thread write independently proven to
            # succeed (found True after write, confirmed by a direct
            # commit) while this exact loop still timed out every time.
            self.env.cr.commit()
            self.group_page.invalidate_recordset(["is_published", "website_published"])
            self.group_post.invalidate_recordset(["is_published"])
            if (
                not self.group_page.is_published
                and not self.group_page.website_published
                and not self.group_post.is_published
            ):
                break
            time.sleep(0.25)  # audit-ignore-sleep

        self.assertFalse(self.group_page.is_published)
        self.assertFalse(self.group_page.website_published)
        self.assertFalse(self.group_post.is_published)

    def test_02_group_pardoning(self):
        self.group.violation_strike_count = 3
        self.group.is_suspended_from_websites = True

        self.group.action_pardon_group_websites()

        self.assertEqual(self.group.violation_strike_count, 0)
        self.assertFalse(self.group.is_suspended_from_websites)
