# -*- coding: utf-8 -*-
# Copyright © Bruce Perens K6BP.
# SPDX-License-Identifier: AGPL-3.0-or-later
import logging
import urllib.error
import odoo.tests
from odoo.tests import tagged
from odoo.addons.zero_sudo.tests.real_transaction import RealTransactionCase
from odoo.exceptions import UserError, ValidationError

_logger = logging.getLogger(__name__)


@tagged("post_install", "-at_install")
class TestAppealsAndViews(RealTransactionCase):

    def setUp(self):
        super(TestAppealsAndViews, self).setUp()
        self.user_public = None
        self.page = None

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
        # Enforce commit to ensure test data is visible to separate HTTP worker threads
        self.env.cr.commit()

    def tearDown(self):
        # Pre-fetch outside the loop to avoid N+1 DB LOCK
        visitors = self.env["website.visitor"].search([], limit=10000)
        tracks = self.env["website.track"].search([], limit=10000)

        # Explicit resilient cleanup to prevent website_visitor/website_track pollution
        for attempt in range(5):
            try:
                with self.env.cr.savepoint():
                    if visitors and visitors.exists():
                        visitors.unlink()
                    if tracks and tracks.exists():
                        tracks.unlink()

                    if self.page and self.page.exists():
                        self.page.unlink()  # Tests [@ANCHOR: user_websites:COMM_website_page_unlink]
                    if self.user_public and self.user_public.exists():
                        self.user_public.unlink()
                break
            except urllib.error.HTTPError as e:
                _logger.warning("Resilient cleanup encountered exception: %s", e)

        self.env.cr.commit()
        super(TestAppealsAndViews, self).tearDown()

    def test_01_privacy_friendly_view_counter(self):
        # Tests [@ANCHOR: test_privacy_friendly_view_counter]

        # Tests [@ANCHOR: procedure_flush_view_counters]
        """Verify the view counter increments cleanly on page load."""
        self.assertEqual(self.page.view_count, 0)

        # Public user visits the page
        self.url_open(f"/{self.user_public.website_slug}/home")

        # Sync snapshot to see the Werkzeug thread's executed updates
        self.env.cr.commit()

        # Flush the redis buffer to postgres
        self.env["website.page"]._flush_redis_view_counters()

        # Raw SQL to aggressively verify the hit registered across ORM caches
        self.env.cr.execute(
            "SELECT view_count FROM website_page WHERE id = %s", [self.page.id]
        )
        count = self.env.cr.fetchone()[0]
        self.assertEqual(count, 1, "View count should increment by 1 on access.")

    def test_02_submit_and_approve_appeal(self):
        # Tests [@ANCHOR: UX_SUBMIT_APPEAL]

        # Tests [@ANCHOR: user_websites:COMM_appeal_action_approve]
        """Verify a suspended user can appeal, and an admin can approve to pardon."""
        # Manually suspend the user
        self.user_public.is_suspended_from_websites = True
        self.env.cr.commit()

        self.authenticate(self.user_public.login, self.user_public.login)

        # User submits an appeal
        self.url_open(
            "/website/submit_appeal",  # burn-ignore-route
            data={
                "csrf_token": odoo.http.Request.csrf_token(self),
                "reason": "It was a misunderstanding!",
            },
            method="POST",
        )

        # Sync snapshot to see the Werkzeug thread's created appeal
        self.env.cr.commit()

        appeal = self.env["content.violation.appeal"].search(
            [("user_id", "=", self.user_public.id)], limit=1
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

    def test_03_reject_appeal_leaves_suspension_in_place(self):
        # Tests [@ANCHOR: user_websites:COMM_appeal_action_reject]

        # Tests [@ANCHOR: user_websites:COMM_check_appeal_target]
        """action_reject() had zero test coverage -- confirm it moves an
        appeal to 'rejected' without pardoning the user. Also proves
        _check_appeal_target()'s own real constraint: an appeal must be
        tied to exactly one of user_id/group_id, not both and not
        neither."""
        self.user_public.is_suspended_from_websites = True
        self.env.cr.commit()

        appeal = self.env["content.violation.appeal"].create(
            {"user_id": self.user_public.id, "reason": "Please reconsider."}
        )
        appeal.action_reject()

        self.assertEqual(appeal.state, "rejected")
        self.assertTrue(
            self.user_public.is_suspended_from_websites,
            "Rejecting an appeal must NOT lift the user's suspension.",
        )

        with self.assertRaises(
            ValidationError,
            msg="An appeal tied to neither a user nor a group must be rejected.",
        ):
            self.env["content.violation.appeal"].create({"reason": "No target"})
            self.env.flush_all()

    def test_04_state_guards_block_reprocessing_a_resolved_appeal(self):
        # Tests [@ANCHOR: user_websites:COMM_appeal_action_approve]

        # Tests [@ANCHOR: user_websites:COMM_appeal_action_reject]
        """
        Bug-hunt regression (2026-09-09): action_approve/action_reject had
        no server-side guard against being invoked on an already-resolved
        appeal -- only a view's own invisible attribute. Re-approving an
        already-approved appeal re-ran the pardon and duplicated audit
        messages; re-rejecting an already-approved appeal overwrote state
        back to 'rejected' and posted a message that was false the moment
        it was made. Prove both now raise instead.
        """
        self.user_public.is_suspended_from_websites = True
        self.env.cr.commit()

        approved = self.env["content.violation.appeal"].create(
            {"user_id": self.user_public.id, "reason": "Approve me once."}
        )
        approved.action_approve()
        self.assertEqual(approved.state, "approved")
        self.assertFalse(self.user_public.is_suspended_from_websites)

        with self.assertRaises(UserError):
            approved.action_approve()
        with self.assertRaises(
            UserError,
            msg="Rejecting an already-approved appeal must not silently "
            "overwrite its state or re-post a now-false message.",
        ):
            approved.action_reject()

        other_user = self.env["res.users"].create(
            {
                "name": "Second Appeal Tester",
                "login": f"appealtester2_{self.id()}",
                "email": "appeal2@example.com",
                "website_slug": f"appealtester2_{self.id()}",
                "is_suspended_from_websites": True,
            }
        )
        rejected = self.env["content.violation.appeal"].create(
            {"user_id": other_user.id, "reason": "Reject me once."}
        )
        rejected.action_reject()
        self.assertEqual(rejected.state, "rejected")
        with self.assertRaises(UserError):
            rejected.action_reject()
        with self.assertRaises(UserError):
            rejected.action_approve()
