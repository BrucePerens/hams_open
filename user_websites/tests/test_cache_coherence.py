# -*- coding: utf-8 -*-
# Copyright © Bruce Perens K6BP. Licensed under the GNU Affero General Public License v3.0 (AGPL-3.0).
import odoo.tests
from odoo.addons.zero_sudo.tests.real_transaction import RealTransactionCase


@odoo.tests.common.tagged("post_install", "-at_install")
class TestCacheCoherence(RealTransactionCase):
    """
    Tests cross-transaction cache invalidation and race conditions
    using real PostgreSQL commits. This closes the 'Single-Transaction Illusion' gap
    where standard tests fail to execute pg_notify invalidations.
    """

    def setUp(self):
        super().setUp()
        self.password = "test_password"

        # Setup User A
        self.user_a = self.env["res.users"].create(
            {
                "name": "User A",
                "login": "user_a_racer",
                "password": self.password,
                "website_slug": "race-test",
                "group_ids": [(6, 0, [self.env.ref("base.group_portal").id])],
            }
        )

        # Commit to ensure data and cache NOTIFY triggers actually fire
        self.env.cr.commit()

    def test_01_slug_reassignment_cache_invalidation(self):
        """
        Action: User A creates a site. Then User A changes their slug to release it.
        User B immediately claims the old slug and creates a new site.
        Expected: The global redis router must immediately flush the cache and
        route traffic to User B without serving a stale 404 or User A's old content.
        """
        svc_uid = self.env["zero_sudo.security.utils"]._get_service_uid(
            "user_websites.user_websites_service_account"
        )

        # 1. User A creates content
        page_a = (
            self.env["website.page"]
            .with_user(svc_uid)
            .create(
                {
                    "url": "/race-test/home",
                    "name": "User A Home",
                    "type": "qweb",
                    "website_published": True,
                    "owner_user_id": self.user_a.id,
                    "arch": '<t name="Home" t-name="user_websites.race_a"><div>USER_A_CONTENT</div></t>',
                }
            )
        )
        self.env.cr.commit()

        # Verify routing goes to User A
        res_a = self.url_open("/race-test/home")
        self.assertEqual(res_a.status_code, 200)
        self.assertIn("USER_A_CONTENT", res_a.content.decode("utf-8"))

        # 2. User A changes their slug to free it up
        self.user_a.write({"website_slug": "race-old"})
        page_a.write({"url": "/race-old/home"})
        self.env.cr.commit()

        # 3. User B immediately claims the released slug
        self.user_b = self.env["res.users"].create(
            {
                "name": "User B",
                "login": "user_b_racer",
                "password": self.password,
                "website_slug": "race-test",
                "group_ids": [(6, 0, [self.env.ref("base.group_portal").id])],
            }
        )

        self.env["website.page"].with_user(svc_uid).create(
            {
                "url": "/race-test/home",
                "name": "User B Home",
                "type": "qweb",
                "website_published": True,
                "owner_user_id": self.user_b.id,
                "arch": '<t name="Home" t-name="user_websites.race_b"><div>USER_B_CONTENT</div></t>',
            }
        )
        self.env.cr.commit()

        # 4. Verify routing goes to User B immediately (Cache was properly invalidated)
        res_b = self.url_open("/race-test/home")
        self.assertEqual(
            res_b.status_code,
            200,
            "Routing failed for re-assigned slug. Suspect caching issue.",
        )

        content_b = res_b.content.decode("utf-8")
        self.assertIn(
            "USER_B_CONTENT",
            content_b,
            "Cache Poisoning Detected: Router returned the old user's page data!",
        )
        self.assertNotIn(
            "USER_A_CONTENT",
            content_b,
            "Cache Poisoning Detected: Router returned the old user's page data!",
        )
