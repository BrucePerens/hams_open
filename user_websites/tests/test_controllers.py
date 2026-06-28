# -*- coding: utf-8 -*-
# Copyright © Bruce Perens K6BP. Licensed under the GNU Affero General Public License v3.0 (AGPL-3.0).
from odoo.tests import tagged
from odoo.addons.zero_sudo.tests.real_transaction import RealTransactionCase


@tagged("post_install", "-at_install")
class TestUserWebsitesControllers(RealTransactionCase):
    """
    Integration tests targeting the HTTP controller layer.
    Enforces multi-persona isolation (Admin, User, Public) and
    validates the REST API contracts defined in the module specification.
    """

    def setUp(self):
        super().setUp()
        self.password = "test_password"

        # Setup Admin Persona
        self.admin = self.env.ref("base.user_admin")
        self.admin.password = self.password

        # Setup Standard User Persona
        self.user_a = self.env["res.users"].create(
            {
                "name": "User A",
                "login": "user_a_http",
                "password": self.password,
                "website_slug": "usera-http",
                "privacy_show_in_directory": True,
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

        # Setup Attacker Persona
        self.attacker = self.env["res.users"].create(
            {
                "name": "Attacker",
                "login": "attacker_http",
                "password": self.password,
                "website_slug": "attacker-http",
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

        # Inject a pending violation report for the API to discover
        self.env["content.violation.report"].with_user(self.user_a).create(
            {
                "target_url": "/some-bad-page",
                "description": "Found a violation",
                "reported_by_user_id": self.user_a.id,
            }
        )

        # Commit to ensure the concurrent HTTP worker can see the database state
        self.env.cr.commit()

    def test_01_api_pending_reports_admin_access(self):
        """
        # [@ANCHOR: test_admin_violation_toast_rpc]
        # Tests [@ANCHOR: admin_toast_logic]
        Tests [@ANCHOR: api_pending_reports]
        Action: Administrator requests the pending reports API.
        Expected: HTTP 200 OK and a valid JSON payload containing the count.
        """
        self.authenticate(self.admin.login, self.password)

        response = self.url_open("/api/v1/user_websites/pending_reports")
        self.assertEqual(
            response.status_code,
            200,
            "Administrator MUST be able to access the pending reports API.",
        )

        data = response.json()
        self.assertIn(
            "count", data, "API contract violation: JSON response missing 'count' key."
        )
        self.assertGreaterEqual(
            data["count"],
            1,
            "API failed to return the correct count of pending reports.",
        )

    def test_02_api_pending_reports_unauthorized_access(self):
        """
        Tests [@ANCHOR: api_pending_reports]
        Action: A standard user and an unauthenticated user attempt to access the admin API.
        Expected: HTTP 200 OK (to avoid UI crash) with an error payload.
        """
        # 1. Test as Standard Authenticated User
        self.authenticate(self.attacker.login, self.password)
        res_user = self.url_open("/api/v1/user_websites/pending_reports")
        self.assertEqual(
            res_user.status_code, 200, "API should return 200 to prevent JS fetch crash"
        )
        self.assertEqual(
            res_user.json().get("error"),
            "Forbidden",
            "API should return Forbidden in JSON payload",
        )

        # 2. Test as Unauthenticated Public Guest
        self.authenticate(None, None)
        res_public = self.url_open("/api/v1/user_websites/pending_reports")
        self.assertEqual(
            res_public.status_code,
            200,
            "API should return 200 to prevent JS fetch crash",
        )
        self.assertEqual(
            res_public.json().get("error"),
            "Forbidden",
            "API should return Forbidden in JSON payload",
        )

    def test_03_community_directory_rendering(self):
        """
        Tests [@ANCHOR: UX_COMMUNITY_DIRECTORY]
        Action: Public user browses the community directory.
        Expected: HTTP 200 OK. The user who opted into `privacy_show_in_directory` MUST be visible.
        """
        self.authenticate(None, None)

        response = self.url_open("/community")
        self.assertEqual(
            response.status_code, 200, "Community directory route is offline."
        )

        content = response.content.decode("utf-8")
        self.assertIn(
            self.user_a.website_slug,
            content,
            "User A opted into the directory but is missing from the rendered HTML.",
        )

    def test_04_community_directory_privacy_respect(self):
        """
        Tests [@ANCHOR: UX_COMMUNITY_DIRECTORY]
        Action: User opts out of the directory.
        Expected: The user's slug MUST NOT appear in the rendered HTML.
        """
        # Attacker explicitly opts out (default behavior, but let's be sure)
        self.attacker.privacy_show_in_directory = False
        self.env.cr.commit()

        self.authenticate(None, None)
        response = self.url_open("/community")
        content = response.content.decode("utf-8")

        self.assertNotIn(
            self.attacker.website_slug,
            content,
            "CRITICAL PRIVACY VIOLATION: User opted out of the directory but was exposed!",
        )
