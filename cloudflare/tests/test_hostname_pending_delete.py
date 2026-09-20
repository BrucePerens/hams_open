# This software is distributed under the terms of the Affero General Public License (AGPL-3).

# -*- coding: utf-8 -*-
"""
`cloudflare.hostname.pending.delete` -- the retry record behind Bruce's
"proceed with the local delete, but keep a retry record" decision (2026-09-19,
`hams_com/night_shift_questions/answered/
cloudflare-hostname-delete-failure-block-or-proceed-05a5144b.md`, "3 sounds
good to me").

Every Cloudflare call is mocked; what is asserted is what the model decided to
do with the answer. The pre-existing WARNING-log test for the same failure path
lives in `test_domain_custom_hostname.py` and is deliberately left alone -- the
log line is still part of the contract, not replaced by this model.
"""
from datetime import timedelta
from unittest.mock import MagicMock

from cryptography.fernet import Fernet
from odoo import fields
from odoo.exceptions import AccessError
from odoo.tests.common import tagged
from odoo.addons.cloudflare.models.hostname_pending_delete import (
    BACKOFF_MINUTES,
    MAX_DELETE_ATTEMPTS,
)
from odoo.addons.cloudflare.utils.cloudflare_api import delete_custom_hostname
from odoo.addons.zero_sudo.tests.real_transaction import RealTransactionCase


@tagged("post_install", "-at_install")
class TestHostnamePendingDelete(RealTransactionCase):
    def setUp(self):
        super().setUp()
        mock_fernet = self.safe_patch(
            "odoo.addons.cloudflare.models.website.WebsiteCloudflare._get_fernet"
        )
        mock_fernet.return_value = Fernet(Fernet.generate_key())

        self.Pending = self.env["cloudflare.hostname.pending.delete"]
        self.website = self.env["website"].create(
            {
                "name": "Pending Delete Test Site",
                "domain": "https://pending-delete-test.example",
            }
        )
        self.website.write(
            {"cloudflare_api_token": "tok", "cloudflare_zone_id": "zone"}
        )
        self.mock_create = self.safe_patch(
            "odoo.addons.cloudflare.models.domain.cf_utils.create_custom_hostname"
        )
        self.mock_create.return_value = (
            True,
            {"id": "hostname_pending", "ssl": {"status": "active"}},
        )

    def _domain(self, slug="pending-delete-1"):
        return self.env["edge.routing.domain"].create(
            {
                "name": "https://pending-delete-test.example",
                "target_slug": slug,
            }
        )

    def _pending_for(self, cf_hostname_id):
        return self.Pending.search([("cf_hostname_id", "=", cf_hostname_id)])

    def test_01_a_failed_cloudflare_delete_creates_a_pending_record(self):
        # Tests [@ANCHOR: COMM_record_failed_hostname_delete]

        # Tests [@ANCHOR: COMM_pending_delete_next_attempt_at]
        """The local row still goes; the orphaned hostname gets a retry record.

        Both halves matter. Blocking the unlink was the rejected option, so the
        `edge.routing.domain` MUST be gone; and the leak MUST no longer be
        log-only, which is what this model exists for.
        """
        domain = self._domain()
        mock_delete = self.safe_patch(
            "odoo.addons.cloudflare.models.domain.cf_utils.delete_custom_hostname"
        )
        mock_delete.return_value = (False, "API Error")

        domain.unlink()

        self.assertFalse(
            domain.exists(),
            "The local routing domain MUST still be deleted -- blocking the "
            "unlink on a Cloudflare hiccup was the explicitly rejected option.",
        )
        pending = self._pending_for("hostname_pending")
        self.assertEqual(len(pending), 1)
        self.assertEqual(pending.state, "pending")
        self.assertEqual(pending.attempt_count, 0)
        self.assertEqual(pending.last_error, "API Error")
        self.assertEqual(pending.website_id, self.website)
        self.assertTrue(
            pending.next_attempt_at,
            "A pending record MUST carry a due time, or the cron's backoff "
            "domain would never select it.",
        )

    def test_02_a_successful_cloudflare_delete_creates_nothing(self):
        # Tests [@ANCHOR: COMM_record_failed_hostname_delete]
        """The happy path must not leave bookkeeping behind."""
        domain = self._domain("pending-delete-2")
        mock_delete = self.safe_patch(
            "odoo.addons.cloudflare.models.domain.cf_utils.delete_custom_hostname"
        )
        mock_delete.return_value = (True, "Custom hostname deleted successfully.")

        domain.unlink()

        self.assertFalse(self._pending_for("hostname_pending"))

    def test_03_a_second_failure_for_the_same_hostname_reuses_one_record(self):
        # Tests [@ANCHOR: COMM_record_failed_hostname_delete]
        """One hostname id is one leak, however many times the delete fails.

        Without this the cron would end up with several rows for the same
        remote object and retry them in parallel against each other.
        """
        pending_model = self.Pending
        first = pending_model._record_failed_delete(
            "https://dup.example", "hostname_dup", self.website, "API Error"
        )
        second = pending_model._record_failed_delete(
            "https://dup.example", "hostname_dup", self.website, "429 Too Many Requests"
        )

        self.assertEqual(first, second)
        self.assertEqual(len(self._pending_for("hostname_dup")), 1)
        self.assertEqual(second.last_error, "429 Too Many Requests")

    def test_04_a_failure_reopens_a_record_that_was_already_closed(self):
        # Tests [@ANCHOR: COMM_record_failed_hostname_delete]
        """A delete that just failed is evidence the hostname still exists."""
        record = self.Pending._record_failed_delete(
            "https://reopen.example", "hostname_reopen", self.website, "API Error"
        )
        record.write({"state": "abandoned", "next_attempt_at": False})

        self.Pending._record_failed_delete(
            "https://reopen.example", "hostname_reopen", self.website, "API Error"
        )

        self.assertEqual(record.state, "pending")
        self.assertTrue(record.next_attempt_at)

    def test_05_the_cron_closes_a_record_when_cloudflare_confirms(self):
        # Tests [@ANCHOR: COMM_cron_retry_pending_hostname_deletes]

        # Tests [@ANCHOR: COMM_pending_delete_attempt]

        # Tests [@ANCHOR: COMM_pending_delete_attempt_one]
        record = self.Pending._record_failed_delete(
            "https://retry-ok.example", "hostname_retry_ok", self.website, "API Error"
        )
        record.next_attempt_at = False
        mock_delete = self.safe_patch(
            "odoo.addons.cloudflare.models.hostname_pending_delete.cf_utils."
            "delete_custom_hostname"
        )
        mock_delete.return_value = (True, "Custom hostname deleted successfully.")

        summary = self.Pending._cron_retry_pending_deletes()

        mock_delete.assert_called_once_with("hostname_retry_ok", "tok", "zone")
        self.assertEqual(summary["done"], 1)
        self.assertEqual(record.state, "done")
        self.assertFalse(record.next_attempt_at)
        self.assertFalse(record.last_error)

    def test_06_a_hostname_that_is_already_gone_counts_as_done(self):
        # Tests [@ANCHOR: COMM_pending_delete_attempt_one]
        """Cloudflare's 404 means the caller's goal is already met.

        Asserted at BOTH layers, because either one alone would let the bug
        back in: `delete_custom_hostname` must map a 404 response to success
        (it previously returned `(False, "API Error")` for every non-200), and
        the model must close the record on that success rather than retrying
        forever against something that is not there.
        """
        fake_response = MagicMock()
        fake_response.status_code = 404
        mock_request = self.safe_patch(
            "odoo.addons.cloudflare.utils.cloudflare_api._make_request"
        )
        mock_request.return_value = fake_response
        self.assertEqual(
            delete_custom_hostname("hostname_gone", "tok", "zone"),
            (True, "Custom hostname was already gone."),
        )

        record = self.Pending._record_failed_delete(
            "https://gone.example", "hostname_gone", self.website, "API Error"
        )
        record.next_attempt_at = False

        self.Pending._cron_retry_pending_deletes()

        self.assertEqual(
            record.state,
            "done",
            "A hostname Cloudflare says is not there MUST close the record.",
        )

    def test_07_repeated_failure_counts_up_and_eventually_stops(self):
        # Tests [@ANCHOR: COMM_pending_delete_fail_attempt]
        """Bounded retries: the record is abandoned, not deleted, at the cap."""
        record = self.Pending._record_failed_delete(
            "https://forever.example", "hostname_forever", self.website, "API Error"
        )
        mock_delete = self.safe_patch(
            "odoo.addons.cloudflare.models.hostname_pending_delete.cf_utils."
            "delete_custom_hostname"
        )
        mock_delete.return_value = (False, "API Error")

        for attempt in range(1, MAX_DELETE_ATTEMPTS + 1):
            record.next_attempt_at = False
            self.Pending._cron_retry_pending_deletes()
            self.assertEqual(record.attempt_count, attempt)

        self.assertEqual(record.state, "abandoned")
        self.assertFalse(record.next_attempt_at)
        self.assertTrue(
            record.exists(),
            "An abandoned record MUST stay visible for an admin -- that is the "
            "whole reason abandoning is not a delete.",
        )

        # The cron must now leave it alone entirely.
        calls_before = mock_delete.call_count
        self.Pending._cron_retry_pending_deletes()
        self.assertEqual(mock_delete.call_count, calls_before)

    def test_08_the_backoff_grows_and_is_capped(self):
        # Tests [@ANCHOR: COMM_pending_delete_next_attempt_at]
        """Later attempts wait longer, and the wait stops growing.

        Also pins the out-of-range case: a count past the table's end must
        clamp rather than raise inside a cron and take every other pending
        record down with it.
        """
        # ONE base time for every call: reading the clock per call would make
        # the clamp assertion below measure drift between two `now()`s instead
        # of the behaviour under test.
        now = fields.Datetime.now()
        first = self.Pending._next_attempt_at(0, now=now)
        second = self.Pending._next_attempt_at(1, now=now)
        capped = self.Pending._next_attempt_at(MAX_DELETE_ATTEMPTS - 1, now=now)
        beyond = self.Pending._next_attempt_at(MAX_DELETE_ATTEMPTS + 99, now=now)

        self.assertEqual(first, now + timedelta(minutes=BACKOFF_MINUTES[0]))
        self.assertEqual(second, now + timedelta(minutes=BACKOFF_MINUTES[1]))
        self.assertLess(first, second)
        self.assertLess(second, capped)
        self.assertEqual(
            beyond,
            capped,
            "A count past the end of the backoff table MUST clamp to the last "
            "entry exactly, not index off it and not keep growing.",
        )

    def test_09_an_admin_can_retry_a_record_by_hand(self):
        # Tests [@ANCHOR: COMM_pending_delete_action_retry_now]

        # Tests [@ANCHOR: COMM_pending_delete_action_abandon]
        """The two buttons on the backend view, including on an abandoned row."""
        record = self.Pending._record_failed_delete(
            "https://manual.example", "hostname_manual", self.website, "API Error"
        )
        record.write({"state": "abandoned", "next_attempt_at": False})
        mock_delete = self.safe_patch(
            "odoo.addons.cloudflare.models.hostname_pending_delete.cf_utils."
            "delete_custom_hostname"
        )
        mock_delete.return_value = (True, "Custom hostname deleted successfully.")

        record.action_retry_now()

        mock_delete.assert_called_once_with("hostname_manual", "tok", "zone")
        self.assertEqual(
            record.state,
            "done",
            "Retry MUST work on an abandoned record -- an admin fixing a "
            "revoked token is exactly the case abandoning exists to wait for.",
        )

        other = self.Pending._record_failed_delete(
            "https://manual2.example", "hostname_manual2", self.website, "API Error"
        )
        other.action_mark_abandoned()
        self.assertEqual(other.state, "abandoned")
        self.assertFalse(other.next_attempt_at)

    def test_10_a_non_admin_can_neither_read_nor_drive_the_model(self):
        # Tests [@ANCHOR: COMM_pending_delete_check_admin]
        """No ACL row grants this model to ordinary users, and the buttons
        re-check before making any credentialed Cloudflare call."""
        record = self.Pending._record_failed_delete(
            "https://acl.example", "hostname_acl", self.website, "API Error"
        )
        portal_user = self.env["res.users"].create(
            {
                "name": "Pending Delete Portal User",
                "login": "pending_delete_portal_user",
                "group_ids": [(6, 0, [self.env.ref("base.group_portal").id])],
            }
        )

        # A real record's read, not `search([])`: an Odoo ACL check fires when
        # there are records to check, so an empty search would prove nothing.
        with self.assertRaises(AccessError):
            record.with_user(portal_user).read(["state"])

        # And the dispatchable public button, which is what an attacker who
        # knows a record id would actually reach -- it must refuse before
        # making any credentialed Cloudflare call.
        mock_delete = self.safe_patch(
            "odoo.addons.cloudflare.models.hostname_pending_delete.cf_utils."
            "delete_custom_hostname"
        )
        with self.assertRaises(AccessError):
            record.with_user(portal_user).action_retry_now()
        mock_delete.assert_not_called()

        with self.assertRaises(AccessError):
            record.with_user(portal_user).action_mark_abandoned()

    def test_11_pending_delete_views_render(self):
        # [@ANCHOR: COMM_test_pending_delete_views_render]

        # Tests [@ANCHOR: COMM_pending_delete_compute_name]
        """The list and form views this module ships actually build.

        The render proof the two `audit-ignore-view` comments in
        `hostname_pending_delete_views.xml` cite, and the display-name compute
        those views lean on for the record's breadcrumb.
        """
        self.env["cloudflare.hostname.pending.delete"].get_view(view_type="list")
        self.env["cloudflare.hostname.pending.delete"].get_view(view_type="form")

        record = self.Pending._record_failed_delete(
            "https://named.example", "hostname_named", self.website, "API Error"
        )
        self.assertEqual(record.name, "https://named.example (hostname_named)")
