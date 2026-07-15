# -*- coding: utf-8 -*-
# Copyright © Bruce Perens K6BP. Licensed under the GNU Affero General Public License v3.0 (AGPL-3.0).
import ast
import odoo
import time
from odoo.tests.common import tagged
from odoo.addons.zero_sudo.tests.common import HamsHttpCase


@tagged("post_install", "-at_install")
class TestSubscriptionsAndDigest(HamsHttpCase):

    def setUp(self):
        super(TestSubscriptionsAndDigest, self).setUp()

        # 1. Create a Content Owner
        self.creator = self.env["res.users"].create(
            {
                "name": "Content Creator",
                "login": "creator_test",
                "email": "creator@example.com",
                "website_slug": "creator-test",
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

        # 2. Create a Follower
        self.follower = self.env["res.users"].create(
            {
                "name": "Enthusiastic Follower",
                "login": "follower_test",
                "email": "follower@example.com",
                "website_slug": "follower-test",
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

        # Subscribe Follower to Creator's Partner
        self.creator.partner_id.message_subscribe(
            partner_ids=[self.follower.partner_id.id]
        )

        # 3. Create a recent post
        blog = self.env["blog.blog"].search([("name", "=", "Community Blog")], limit=1)
        if not blog:
            blog = self.env["blog.blog"].create({"name": "Community Blog"})

        self.env["blog.post"].create(
            {
                "name": "My New Weekly Recipe",
                "blog_id": blog.id,
                "owner_user_id": self.creator.id,
                "is_published": True,
                "website_published": True,  # Explicitly set mixin field for testing environments
            }
        )

    def test_01_weekly_digest_and_unsubscribe_headers(self):
        """
        Verify that the cron correctly generates emails, successfully injects the
        List-Unsubscribe headers, and that the unsubscribe route works.
        """
        # [@ANCHOR: test_weekly_digest_secret]

        # Tests [@ANCHOR: send_weekly_digest]

        # [@ANCHOR: COMM_test_weekly_digest_mail_template]

        # Tests [@ANCHOR: send_weekly_digest]

        # [@ANCHOR: test_unsubscribe_secret]

        # Tests [@ANCHOR: controller_unsubscribe_digest]

        # Execute the cron job method directly
        self.env["blog.post"].send_weekly_digest()

        # Find the generated email natively linked to the recipient partner
        # We search as the mail service account to bypass any restrictive rules and
        # ensure we see the records generated in the shared transaction.
        mail_svc_uid = self.env["zero_sudo.security.utils"]._get_service_uid(
            "zero_sudo.mail_service_internal"
        )
        mail = (
            self.env["mail.mail"]
            .with_user(mail_svc_uid)
            .search(
                [
                    ("recipient_ids", "in", [self.follower.partner_id.id]),
                    ("subject", "ilike", "Weekly Update from Content Creator"),
                ],
                limit=1,
            )
        )

        self.assertTrue(
            mail,
            "The system must generate a mail.mail record for the follower. [!] DIAGNOSTIC FOR AI: mail.mail search returned empty. This often happens in RealTransactionCase if the mail queue is not properly inspected or if ACLs prevent the test user from seeing the system-generated email.",
        )

        # Assert Service Account Role Execution
        mail_svc = self.env["zero_sudo.security.utils"]._get_service_uid(
            "zero_sudo.mail_service_internal"
        )
        self.assertEqual(
            mail.create_uid.id,
            mail_svc,
            "Email generation MUST execute strictly under the Mail Service Account.",
        )

        # Extract headers (FIXED: Replaced dangerous eval() with safe ast.literal_eval)
        headers_dict = ast.literal_eval(mail.headers) if mail.headers else {}
        self.assertIn(
            "List-Unsubscribe",
            headers_dict,
            "The email must contain the List-Unsubscribe header.",
        )
        self.assertIn(
            "List-Unsubscribe-Post",
            headers_dict,
            "The email must contain the List-Unsubscribe-Post header.",
        )

        # Extract the unsubscribe URL from the header (it's wrapped in angle brackets)
        unsub_url_raw = headers_dict["List-Unsubscribe"]
        unsub_url = unsub_url_raw.strip("<>")

        self.assertTrue(
            "/website/unsubscribe/res.partner/" in unsub_url,  # burn-ignore-route
            "The URL must map to the correct controller route.",
        )

        # Verify the follower is currently subscribed
        self.assertIn(
            self.follower.partner_id,
            self.creator.partner_id.message_follower_ids.mapped("partner_id"),
        )

        # Simulate hitting the unsubscribe URL via an unauthenticated session (public user)
        self.authenticate(None, None)
        # Extract just the path for url_open (stripping the base_url)
        path = unsub_url.split(
            self.env["ir.config_parameter"].get_param("web.base.url")
        )[-1]

        response = self.url_open(path)
        self.assertEqual(
            response.status_code,
            200,
            "The unsubscribe route should render a success page.",
        )
        self.assertIn(b"Unsubscribed Successfully", response.content)

        # Verify the follower has actually been removed
        self.creator.partner_id.invalidate_recordset(["message_follower_ids"])
        self.assertNotIn(
            self.follower.partner_id,
            self.creator.partner_id.message_follower_ids.mapped("partner_id"),
            "The follower must be removed from the record after accessing a valid unsubscribe link.",
        )

        template = self.env.ref(
            "user_websites.email_template_weekly_digest", raise_if_not_found=False
        )
        if template:
            test_post = self.env["blog.post"].search(
                [("owner_user_id", "=", self.creator.id)], limit=1
            )
            template.send_mail(test_post.id, force_send=False)   # Tested by [@ANCHOR: COMM_test_weekly_digest_mail_template]

    def test_02_invalid_unsubscribe_token(self):
        """
        Ensure that malicious actors cannot spoof the unsubscription URL to
        force-remove other users from mailing lists.
        """
        # Attempt an unsubscribe with a forged token
        fake_token = "1234abcd5678"
        current_ts = int(time.time())
        # burn-ignore-route
        url = f"/website/unsubscribe/res.partner/{self.creator.partner_id.id}/{self.follower.partner_id.id}/{current_ts}/{fake_token}"

        self.authenticate(None, None)
        response = self.url_open(url)

        self.assertEqual(
            response.status_code,
            403,
            "The controller must return a 403 Forbidden for invalid tokens.",
        )

        # Verify the user is STILL subscribed
        self.creator.partner_id.invalidate_recordset(["message_follower_ids"])
        self.assertIn(
            self.follower.partner_id,
            self.creator.partner_id.message_follower_ids.mapped("partner_id"),
            "The follower must not be removed if the token is invalid.",
        )


def test_03_subscribe_to_site(self):
    # [@ANCHOR: test_subscribe_to_site]

    # [@ANCHOR: test_subscription_creation]

    # Tests [@ANCHOR: UX_SUBSCRIBE]
    """
    Verify that users can subscribe to a site.
    """
    self.authenticate(self.follower.login, self.follower.login)
    response = self.url_open(
        f"/{self.creator.website_slug}/subscribe",
        data={"csrf_token": odoo.http.Request.csrf_token(self)},
        method="POST",
    )
    self.assertEqual(response.status_code, 200)
