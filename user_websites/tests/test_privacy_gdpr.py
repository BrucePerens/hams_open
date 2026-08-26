# -*- coding: utf-8 -*-
# Copyright © Bruce Perens K6BP. Licensed under the GNU Affero General Public License v3.0 or later (AGPL-3.0-or-later).
import odoo
import odoo.tests
from odoo import fields
from odoo.tests import tagged
from odoo.addons.zero_sudo.tests.common import HamsHttpCase
from urllib.parse import unquote
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

    def test_01a_export_zip_route_redirects_with_a_token(self):
        # [@ANCHOR: test_gdpr_export_zip_redirect]
        """/my/privacy/export.zip mints a token and redirects to the daemon's
        own download endpoint, per docs/proposals/GDPR_CSV_EXPORT.md -- this
        controller itself never streams the zip, it only hands off."""
        # Tests [@ANCHOR: gdpr_export_token]
        self.authenticate(self.user_privacy.login, self.user_privacy.login)
        response = self.url_open(
            "/my/privacy/export.zip", allow_redirects=False
        )
        self.assertEqual(
            response.status_code, 303,
            "The export.zip route must redirect (Odoo's http redirect()), not serve the zip itself.",
        )
        location = response.headers.get("Location", "")
        self.assertIn("/api/v1/gdpr_export/download?token=", location)
        token = location.split("token=")[-1]
        self.assertTrue(token, "A real, non-empty token must be present in the redirect URL.")

        # The token this route minted must actually exist, be tied to this
        # user, and be unconsumed -- verified at the ORM level, not just
        # that the redirect URL has *a* token-shaped string in it.
        record = self.env["ham.gdpr.export.token"].sudo().search(
            [("token", "=", token)], limit=1
        )
        self.assertTrue(record, "The minted token must exist as a real ham.gdpr.export.token row.")
        self.assertEqual(record.user_id, self.user_privacy)
        self.assertFalse(record.consumed)

    def test_01b_export_token_is_single_use(self):
        # [@ANCHOR: test_gdpr_export_token_single_use]
        """_consume() must refuse a second attempt against the same token --
        this is what makes the daemon-facing handoff token non-replayable."""
        env_as_user = self.env(user=self.user_privacy)
        token = env_as_user["ham.gdpr.export.token"].create_for_current_user()

        Token = self.env["ham.gdpr.export.token"].sudo()
        user = Token._consume(token)
        self.assertEqual(user, self.user_privacy)

        with self.assertRaises(odoo.exceptions.AccessError):
            Token._consume(token)

    def test_01c_export_token_rejects_unknown_token(self):
        # [@ANCHOR: test_gdpr_export_token_unknown_rejected]
        with self.assertRaises(odoo.exceptions.AccessError):
            self.env["ham.gdpr.export.token"].sudo()._consume("not-a-real-token")

    def test_01d_export_token_rejects_expired_token(self):
        # [@ANCHOR: test_gdpr_export_token_expiry]
        """A token older than TOKEN_EXPIRY_MINUTES must be refused even
        though it was never consumed -- the narrow expiry window is a real
        security property, not just a single-use guarantee."""
        from odoo.addons.user_websites.models.ham_gdpr_export_token import TOKEN_EXPIRY_MINUTES
        from datetime import timedelta

        Token = self.env["ham.gdpr.export.token"].sudo()
        record = Token.create({"user_id": self.user_privacy.id, "token": "an-old-token"})
        # create_date is an Odoo-managed magic column that a plain write()
        # silently ignores -- backdating it for real needs a direct SQL
        # UPDATE, matching how this codebase's other tests manipulate
        # magic columns when a real elapsed-time condition must be tested.
        stale = fields.Datetime.now() - timedelta(minutes=TOKEN_EXPIRY_MINUTES + 1)
        self.env.cr.execute(
            "UPDATE ham_gdpr_export_token SET create_date = %s WHERE id = %s",
            (stale, record.id),
        )
        record.invalidate_recordset(["create_date"])

        with self.assertRaises(odoo.exceptions.AccessError):
            Token._consume("an-old-token")

    def test_01e_consume_and_export_materializes_data_and_streamed_keys(self):
        # [@ANCHOR: test_gdpr_consume_and_export_payload]
        """The one RPC entrypoint the daemon calls -- must return the same
        underlying data _get_gdpr_export_data()/_get_gdpr_streamed_keys()
        already produce, with streamed generators fully drained into
        JSON-serializable lists, and the token consumed as a side effect.
        Called the same way the real daemon calls it over RPC: as the
        gdpr_export_service_internal service account, not as the exporting
        user's own session and not via a blanket .sudo() -- consume_and_export
        is the one place internal sudo() use is deliberately scoped, per its
        own docstring, and this test exercises that real boundary."""
        env_as_user = self.env(user=self.user_privacy)
        token = env_as_user["ham.gdpr.export.token"].create_for_current_user()

        svc_uid = self.env["zero_sudo.security.utils"]._get_service_uid(
            "user_websites.user_gdpr_export_service"
        )
        payload = self.env["ham.gdpr.export.token"].with_user(svc_uid).consume_and_export(token)

        self.assertIn("data", payload)
        self.assertIn("streamed", payload)
        self.assertEqual(payload["data"]["user"]["name"], "Privacy User")
        # "pages" is one of _get_gdpr_streamed_keys()'s generator-backed
        # keys (user_websites/models/res_users.py), not one of
        # _get_gdpr_export_data()'s plain dict keys -- consume_and_export
        # deliberately keeps the two separate rather than merging them the
        # way the JSON export's own generate() does, so this checks the
        # generator was actually drained into a real, non-empty list, not
        # just that the key exists.
        self.assertIn("pages", payload["streamed"])
        self.assertIsInstance(payload["streamed"]["pages"], list)
        self.assertEqual(len(payload["streamed"]["pages"]), 1)
        self.assertEqual(payload["streamed"]["pages"][0]["name"], "My Private Home")

        record = self.env["ham.gdpr.export.token"].sudo().search([("token", "=", token)], limit=1)
        self.assertTrue(record.consumed, "consume_and_export must consume the token as a side effect.")

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
        # Erasure now also deactivates the account (ham_onboarding's
        # _execute_gdpr_erasure, folded into this same call chain), so the
        # session is invalid by the time the browser follows the controller's
        # redirect to /my/home?erased=1 -- it bounces once more to the login
        # page with that original URL percent-encoded in ?redirect=. Decode
        # before checking, rather than a literal "erased=1" substring match.
        self.assertIn(
            "erased=1",
            unquote(response.url),
            "Must safely redirect upon deletion (directly, or via a login "
            "bounce if the erasure also deactivated the session).",
        )
        self.user_privacy.invalidate_recordset(["active"])
        self.assertFalse(
            self.user_privacy.sudo().active,
            "A fully-erased account must be deactivated, not just stripped of content.",
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

    def test_03_self_writeable_fields_idiom(self):
        # Tests [@ANCHOR: test_user_websites_self_writeable_fields]
        """
        MASTER_10 Identity & Access Control, section 2: a
        SELF_WRITEABLE_FIELDS override must be proven by an actual
        non-admin self-write, not just a list-membership check. Verify a
        standard portal user (not admin) can write their own
        privacy_show_in_directory preference via Odoo's native self-write
        bypass.
        """
        self.assertIn(
            "privacy_show_in_directory",
            self.env["res.users"].SELF_WRITEABLE_FIELDS,
        )
        self.user_privacy.with_user(self.user_privacy).write(
            {"privacy_show_in_directory": False}
        )
        self.assertFalse(self.user_privacy.privacy_show_in_directory)
