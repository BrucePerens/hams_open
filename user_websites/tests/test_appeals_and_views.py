# -*- coding: utf-8 -*-
# Copyright © Bruce Perens K6BP. Licensed under the GNU Affero General Public License v3.0 (AGPL-3.0).
import odoo.tests


@odoo.tests.common.tagged("post_install", "-at_install")
class TestAppealsAndViews(odoo.tests.common.HttpCase):

    def setUp(self):
        super(TestAppealsAndViews, self).setUp()

        self.user_public = self.env["res.users"].create(
            {
                "name": "Appeal Tester",
                "login": "appealtester",
                "email": "appeal@example.com",
                "website_slug": "appealtester",
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

        self.page = self.env["website.page"].create(
            {
                "url": f"/{self.user_public.website_slug}/home",
                "name": "Home",
                "type": "qweb",
                "arch": '<t name="Home" t-name="home"><t t-call="website.layout"><div>Test</div></t></t>',
                "website_published": True,
                "owner_user_id": self.user_public.id,
            }
        )

    def test_01_privacy_friendly_view_counter(self):
        """Verify the view counter increments cleanly on page load."""
        self.assertEqual(self.page.view_count, 0)

        # Public user visits the page
        self.url_open(f"/{self.user_public.website_slug}/home")

        # Reload record to check updated count
        self.page.invalidate_recordset()
        self.assertEqual(
            self.page.view_count, 1, "View count should increment by 1 on access."
        )

    def test_02_submit_and_approve_appeal(self):
        # Tests [@ANCHOR: UX_SUBMIT_APPEAL]
        """Verify a suspended user can appeal, and an admin can approve to pardon."""
        # Manually suspend the user
        self.user_public.is_suspended_from_websites = True

        self.authenticate(self.user_public.login, self.user_public.login)

        # User submits an appeal
        self.url_open(
            "/website/submit_appeal", # burn-ignore-route
            data={
                "csrf_token": odoo.http.Request.csrf_token(self),
                "reason": "It was a misunderstanding!",
            },
            method="POST",
        )

        appeal = self.env["content.violation.appeal"].search(
            [("user_id", "=", self.user_public.id)]
        )
        self.assertTrue(appeal, "Appeal record should be created.")
        self.assertEqual(appeal.state, "new")

        # Admin processes the appeal
        self.authenticate("admin", "admin")
        appeal.action_approve()

        self.assertEqual(
            appeal.state, "approved", "State should be updated to approved."
        )
        self.assertFalse(
            self.user_public.is_suspended_from_websites,
            "User should be automatically pardoned.",
        )
