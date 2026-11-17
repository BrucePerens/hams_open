# -*- coding: utf-8 -*-
# Copyright © Bruce Perens K6BP. Licensed under the GNU Affero General Public License v3.0 or later (AGPL-3.0-or-later).
import ast
import datetime
import hashlib
import hmac
import odoo
import time
from werkzeug.exceptions import NotFound
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

    # Tests [@ANCHOR: send_weekly_digest]
    def test_01b_weekly_digest_reaches_the_follower_once(self):
        """The digest names the follower once (recipient_ids); a second To: address would send it twice."""
        self.env["blog.post"].send_weekly_digest()
        mail_svc_uid = self.env["zero_sudo.security.utils"]._get_service_uid("zero_sudo.mail_service_internal")
        mails = self.env["mail.mail"].with_user(mail_svc_uid).search(
            [("recipient_ids", "in", [self.follower.partner_id.id]), ("subject", "ilike", "Weekly Update from Content Creator")]
        )
        self.assertTrue(mails)
        self.assertFalse(any(mails.mapped("email_to")), "recipient_ids plus email_to makes Odoo send two copies")

    # [@ANCHOR: user_websites:test_weekly_digest_one_bad_subscriber]
    # Tests [@ANCHOR: send_weekly_digest]
    def test_01e_weekly_digest_one_bad_subscriber_does_not_block_the_rest(self):
        """A send_mail failure for one subscriber is skipped; the others are still attempted."""
        second = self.env["res.users"].create(
            {
                "name": "Second Follower",
                "login": "follower2_test",
                "email": "follower2@example.com",
                "website_slug": "follower2-test",
                "group_ids": [(6, 0, [self.env.ref("base.group_portal").id])],
            }
        )
        self.creator.partner_id.message_subscribe(partner_ids=[second.partner_id.id])
        calls = []

        def fail_first(*args, **kwargs):
            calls.append(1)
            if len(calls) == 1:
                raise ValueError("boom")
            return True

        self.safe_patch("odoo.addons.mail.models.mail_template.MailTemplate.send_mail", side_effect=fail_first)
        self.env["blog.post"].send_weekly_digest()
        self.assertGreaterEqual(len(calls), 2, "the failing subscriber must not stop the later ones")

    # Tests [@ANCHOR: user_websites_sql_views_user_websites_weekly_digest_view]
    def test_01f_weekly_digest_skips_a_deactivated_or_erased_subscriber(self):
        """An erased member keeps the partner and its follower rows; the digest must not mail them."""
        self.follower.write({"active": False})
        self.env["blog.post"].send_weekly_digest()
        mail_svc_uid = self.env["zero_sudo.security.utils"]._get_service_uid("zero_sudo.mail_service_internal")
        mails = self.env["mail.mail"].with_user(mail_svc_uid).search(
            [("recipient_ids", "in", [self.follower.partner_id.id])]
        )
        self.assertFalse(mails, "a deactivated subscriber must get no digest")

    # Tests [@ANCHOR: user_websites_sql_views_user_websites_weekly_digest_view]
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
        self.env["blog.post"].send_weekly_digest()  # Tests [@ANCHOR: user_websites:COMM_weekly_digest_view_init]

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
            "/website/unsubscribe/res.partner/" in unsub_url,
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
            template.send_mail(test_post.id, force_send=False)   # audit-ignore-mail: model_id verified against user_websites_data.xml (website_blog.model_blog_post, matches blog.post) -- Tested by [@ANCHOR: COMM_test_weekly_digest_mail_template]

    def test_01b_weekly_digest_view_excludes_a_post_older_than_7_days_in_real_utc_terms(self):
        # user_websites_weekly_digest_view.md, finding 2:
        # `create_date >= now() - interval '7 days'` compared a naive (UTC-per-Odoo-
        # convention) timestamp column against `now()` (timestamptz), which forces
        # PostgreSQL to implicitly reinterpret the naive value in the SESSION's own
        # TimeZone GUC (e.g. America/Los_Angeles) rather than as
        # UTC -- widening the effective window by up to the session's own UTC offset
        # (7-8 hours). A post genuinely more than 7 days old in real UTC terms (here,
        # 7 days + 3 hours) must not be included. This is done by converting `now()`
        # to a naive UTC timestamp via `AT TIME ZONE 'UTC'` before subtracting, making
        # both sides of the comparison naive (no implicit session-timezone cast).
        # Tests [@ANCHOR: user_websites:COMM_weekly_digest_view_init]
        stale_post = self.env["blog.post"].search(
            [("owner_user_id", "=", self.creator.id)], limit=1
        )
        stale_create_date = odoo.fields.Datetime.now() - datetime.timedelta(
            days=7, hours=3
        )
        self.env.cr.execute(
            "UPDATE blog_post SET create_date = %s WHERE id = %s",
            (stale_create_date, stale_post.id),
        )
        stale_post.invalidate_recordset(["create_date"])

        digests = self.env["user_websites.weekly_digest_view"].search(
            [("partner_id", "=", self.follower.partner_id.id)]
        )
        for digest in digests:
            self.assertNotIn(
                str(stale_post.id),
                (digest.post_ids_string or "").split(","),
                "[!] DIAGNOSTIC FOR AI: a post created more than 7 days ago in real UTC "
                "terms must never appear in the weekly digest view, regardless of the "
                "Postgres session's own TimeZone setting.",
            )

    def test_01d_weekly_digest_view_ids_do_not_shift_when_a_subscriber_joins(self):
        # Tests [@ANCHOR: user_websites:COMM_weekly_digest_view_init]
        # The send walks the view in batches by `id > last_digest_id`; an id that moves when another row appears
        # skips one subscriber's digest or repeats another's.
        partners = self.env["res.partner"].create([{"name": "Digest A"}, {"name": "Digest B"}])
        early, late = partners[0], partners[1]
        self.creator.partner_id.message_subscribe(partner_ids=[late.id])
        view = self.env["user_websites.weekly_digest_view"]
        before = view.search([("partner_id", "=", late.id)]).mapped("id")
        self.assertEqual(len(before), 1)
        self.creator.partner_id.message_subscribe(partner_ids=[early.id])
        after = view.search([("partner_id", "=", late.id)]).mapped("id")
        self.assertEqual(after, before, "[!] DIAGNOSTIC FOR AI: a digest row's id moved when another subscriber joined.")

    def test_01c_weekly_digest_view_ids_are_stable_across_repeated_queries(self):
        # user_websites_weekly_digest_view.md, finding 1:
        # `row_number() OVER ()` (no ORDER BY inside the window) numbered rows in
        # whatever order Postgres's hash-aggregate happened to produce, which is not
        # guaranteed stable across independent executions of the same view SQL -- and
        # since this is a plain (non-materialized) VIEW, `search()` and the later
        # `_read()` triggered by the first field access are each a SEPARATE execution.
        # The view adds `ORDER BY f.partner_id, u.partner_id` (and the group branch's
        # `f.partner_id, g.id`) inside the window function, making the id assignment a
        # deterministic function of the GROUP BY key rather than of aggregation
        # internals. This test creates a second, independent digest row (a second
        # follower/owner pair) so there is more than one row whose relative order could
        # shift, then queries the view twice independently and asserts the same id
        # maps to the same owner both times.
        # Tests [@ANCHOR: user_websites:COMM_weekly_digest_view_init]
        second_creator = self.env["res.users"].create(
            {
                "name": "Second Content Creator",
                "login": "creator_test_2",
                "email": "creator2@example.com",
                "website_slug": "creator-test-2",
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
        second_creator.partner_id.message_subscribe(
            partner_ids=[self.follower.partner_id.id]
        )
        blog = self.env["blog.blog"].search([("name", "=", "Community Blog")], limit=1)
        self.env["blog.post"].create(
            {
                "name": "Second Creator's Post",
                "blog_id": blog.id,
                "owner_user_id": second_creator.id,
                "is_published": True,
                "website_published": True,
            }
        )

        View = self.env["user_websites.weekly_digest_view"]
        first_pass = View.search([("partner_id", "=", self.follower.partner_id.id)])
        first_mapping = {d.id: d.owner_record_id for d in first_pass}
        self.assertGreaterEqual(
            len(first_mapping), 2, "fixture: at least 2 distinct digest rows required"
        )

        View.invalidate_model()
        second_pass = View.search([("partner_id", "=", self.follower.partner_id.id)])
        second_mapping = {d.id: d.owner_record_id for d in second_pass}

        self.assertEqual(
            first_mapping,
            second_mapping,
            "[!] DIAGNOSTIC FOR AI: the same digest view id must map to the same "
            "owner_record_id across independent executions of the underlying SQL -- "
            "an unstable row_number() would let a later field-read return a "
            "different conceptual row than search() originally identified by that id.",
        )

    def test_02_invalid_unsubscribe_token(self):
        # Tests [@ANCHOR: user_websites:COMM_unsubscribe]
        """
        Ensure that malicious actors cannot spoof the unsubscription URL to
        force-remove other users from mailing lists.
        """
        # Attempt an unsubscribe with a forged token
        fake_token = "1234abcd5678"
        current_ts = int(time.time())
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


    def test_02b_one_click_post_unsubscribes(self):
        # Tests [@ANCHOR: user_websites:COMM_unsubscribe]
        """The digest's List-Unsubscribe-Post header promises a session-less, token-less POST; it must work."""
        utils = self.env["zero_sudo.security.utils"]
        creator, follower = self.creator.partner_id.id, self.follower.partner_id.id
        ts = int(time.time())
        message = f"res.partner-{creator}-{follower}-{ts}".encode()
        token = hmac.new(utils._get_crypto_secret().encode(), message, hashlib.sha256).hexdigest()
        self.authenticate(None, None)
        response = self.url_open(
            f"/website/unsubscribe/res.partner/{creator}/{follower}/{ts}/{token}",
            data={"List-Unsubscribe": "One-Click"},
        )
        self.assertEqual(response.status_code, 200)
        self.creator.partner_id.invalidate_recordset(["message_follower_ids"])
        self.assertNotIn(self.follower.partner_id, self.creator.partner_id.message_follower_ids.partner_id)

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

    def test_03b_a_site_that_is_not_public_cannot_be_subscribed_to(self):
        # Tests [@ANCHOR: UX_SUBSCRIBE]
        """A suspended member's site answers 404 everywhere; subscribe must not confirm it exists or add a follower."""
        self.creator.partner_id.message_unsubscribe([self.follower.partner_id.id])  # setUp subscribed the follower
        self.creator.is_suspended_from_websites = True
        self.env.flush_all()
        self.authenticate(self.follower.login, self.follower.login)
        response = self.url_open(
            f"/{self.creator.website_slug}/subscribe",
            data={"csrf_token": odoo.http.Request.csrf_token(self)},
            method="POST",
        )
        self.assertEqual(response.status_code, 404)
        self.creator.partner_id.invalidate_recordset(["message_follower_ids"])
        self.assertNotIn(self.follower.partner_id, self.creator.partner_id.message_follower_ids.partner_id)

    def test_unsubscribe_route_takes_a_ten_digit_timestamp_but_no_oversized_id(self):
        # Tests [@ANCHOR: user_websites:COMM_unsubscribe]
        # Every <int:x> route refuses a number past a database id (hams_base BoundedIntConverter); the signed link's
        # timestamp is the one number that may be larger, so its own bound must stay explicit on the route.
        adapter = self.env["ir.http"].routing_map().bind("odoo")
        rule, _args = adapter.match("/website/unsubscribe/res.partner/1/1/4000000000/token", method="GET", return_rule=True)
        self.assertEqual(rule.rule.split("/")[2], "unsubscribe")
        with self.assertRaises(NotFound):
            adapter.match("/website/unsubscribe/res.partner/99999999999/1/4000000000/token", method="GET")
