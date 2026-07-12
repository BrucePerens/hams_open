# This software is distributed under the terms of the Affero General Public License (AGPL-3).

# -*- coding: utf-8 -*-
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
        # audit-ignore-sleep: Give background thread time to unpublish
        time.sleep(1.0)

        # Verify suspension
        self.assertTrue(self.group.is_suspended_from_websites)

        # Verify content is unpublished
        self.group_page.invalidate_recordset(["is_published", "website_published"])
        self.group_post.invalidate_recordset(["is_published"])

        self.assertFalse(self.group_page.is_published)
        self.assertFalse(self.group_page.website_published)
        self.assertFalse(self.group_post.is_published)

    def test_02_group_pardoning(self):
        self.group.violation_strike_count = 3
        self.group.is_suspended_from_websites = True

        self.group.action_pardon_group_websites()

        self.assertEqual(self.group.violation_strike_count, 0)
        self.assertFalse(self.group.is_suspended_from_websites)
