# -*- coding: utf-8 -*-
# Copyright © Bruce Perens K6BP. Licensed under the GNU Affero General Public License v3.0 (AGPL-3.0).
import odoo
import odoo.tests
from odoo.tests import tagged
from odoo.addons.zero_sudo.tests.common import HamsHttpCase
import json


@tagged("post_install", "-at_install")
class TestPrivacyGDPR(HamsHttpCase):

    def setUp(self):
        super(TestPrivacyGDPR, self).setUp()

        self.user_privacy = self.env["res.users"].create(
            {
                "name": "Privacy User",
                "login": "privacy_tester",
                "email": "privacy@example.com",
                "website_slug": "privacy-tester",
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

        # Create some test data for the user
        self.page = self.env["website.page"].create(
            {
                "url": f"/{self.user_privacy.website_slug}/home",
                "name": "My Private Home",
                "type": "qweb",
                "arch": "<div>Sensitive Text</div>",
                "owner_user_id": self.user_privacy.id,
            }
        )

        blog = self.env["blog.blog"].search([("name", "=", "Community Blog")], limit=1)
        if not blog:
            blog = self.env["blog.blog"].create({"name": "Community Blog"})

        self.post = self.env["blog.post"].create(
            {
                "name": "My Journal",
                "blog_id": blog.id,
                "content": "Journal entry details.",
                "owner_user_id": self.user_privacy.id,
            }
        )
        self.env.flush_all()

    def test_01_data_portability_export(self):
        # [@ANCHOR: test_gdpr_export_api]

        # Tests [@ANCHOR: UX_GDPR_EXPORT]
        """Verify the user can successfully download a JSON payload of their data."""
        self.authenticate(self.user_privacy.login, self.user_privacy.login)

        # Hit the export route
        self.env.flush_all()
        response = self.url_open(
            "/my/privacy/export",
            data={"csrf_token": odoo.http.Request.csrf_token(self)},
            method="POST",
        )

        self.assertEqual(
            response.status_code, 200, "The export route must return 200 OK."
        )
        self.assertIn(
            "application/json",
            response.headers.get("Content-Type", ""),
            "Response must be JSON formatted.",
        )
        self.assertIn(
            "attachment",
            response.headers.get("Content-Disposition", ""),
            "Response must prompt a file download.",
        )

        # Parse the JSON response and assert data accuracy
        exported_data = json.loads(response.content)

        self.assertEqual(exported_data["user"]["name"], "Privacy User")

        # Check that the page was exported
        self.assertEqual(len(exported_data["pages"]), 1)
        self.assertEqual(exported_data["pages"][0]["name"], "My Private Home")

        # Check that blog was exported
        self.assertEqual(len(exported_data["blog_posts"]), 1)
        self.assertEqual(exported_data["blog_posts"][0]["name"], "My Journal")

        # Check that reports and appeals were exported (even if empty, keys must exist)
        self.assertIn("submitted_reports", exported_data)
        self.assertIn("appeals", exported_data)

    def test_02_right_to_erasure(self):
        """Verify the user can permanently hard-delete their authored content and opt-out of directories."""
        self.authenticate(self.user_privacy.login, self.user_privacy.login)

        # Ensure data exists initially
        self.assertTrue(self.page.exists())
        self.assertTrue(self.post.exists())
        self.assertTrue(self.user_privacy.privacy_show_in_directory)

        # Trigger Erasure
        # [@ANCHOR: test_gdpr_erasure_pages]

        # Tests [@ANCHOR: gdpr_sudo_erasure]

        # [@ANCHOR: test_gdpr_erasure_posts]

        # Tests [@ANCHOR: gdpr_sudo_erasure]

        # Tests [@ANCHOR: UX_GDPR_ERASURE]
        self.env.flush_all()
        response = self.url_open(
            "/my/privacy/delete_content",
            data={"csrf_token": odoo.http.Request.csrf_token(self)},
            method="POST",
        )

        self.assertEqual(response.status_code, 200)
        self.assertIn(
            b"erased=1", response.url.encode(), "Must safely redirect upon deletion."
        )

        # Re-check the database to prove the records were unlinked
        self.assertFalse(
            self.page.exists(), "The user's website pages must be permanently deleted."
        )
        self.assertFalse(
            self.post.exists(), "The user's blog posts must be permanently deleted."
        )

        # Prove they were opted out of the directory
        self.user_privacy.invalidate_recordset(["privacy_show_in_directory"])
        self.assertFalse(
            self.user_privacy.privacy_show_in_directory,
            "User must be removed from the public directory.",
        )
