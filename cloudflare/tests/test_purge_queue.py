# Copyright © Bruce Perens K6BP.
# SPDX-License-Identifier: AGPL-3.0-or-later

# -*- coding: utf-8 -*-
import logging
from unittest.mock import MagicMock
from odoo.tests.common import tagged
from odoo.addons.zero_sudo.tests.real_transaction import RealTransactionCase
from odoo.models import BaseModel

_logger = logging.getLogger(__name__)


@tagged("post_install", "-at_install")
class TestPurgeQueue(RealTransactionCase):
    def setUp(self):
        super().setUp()
        self.env["cloudflare.purge.queue"].search([], limit=10000).unlink()
        self.website = self.env["website"].get_current_website()
        self.website.write(
            {"cloudflare_api_token": "fake_token", "cloudflare_zone_id": "fake_zone"}
        )

    def test_01_bdd_queue_batching_and_rate_limiting(self):
        # [@ANCHOR: COMM_test_queue_batching_and_rate_limiting]

        # Tests [@ANCHOR: COMM_ir_cron_process_cf_purge_queue]

        # Tests [@ANCHOR: COMM_cf_process_queue_logic]
        mock_post = self.safe_patch(
            "odoo.addons.cloudflare.utils.cloudflare_api.session.post"
        )
        mock_sleep = self.safe_patch("time.sleep")

        mock_response = MagicMock()
        mock_response.status_code = 200
        mock_post.return_value = mock_response

        QueueModel = self.env["cloudflare.purge.queue"]
        vals = []
        for i in range(310):
            vals.append(
                {
                    "target_item": f"https://example.com/page-{i}",
                    "purge_type": "url",
                    "website_id": self.website.id,
                }
            )
        QueueModel.create(vals)
        self.assertEqual(QueueModel.search_count([]), 310)

        cron = self.env.ref(
            "cloudflare.ir_cron_process_cf_purge_queue", raise_if_not_found=False
        )

        mock_trigger = self.safe_patch_object(type(cron), "_trigger")

        QueueModel.process_queue()

        self.assertEqual(mock_post.call_count, 10, "MUST batch exactly 10 requests.")
        self.assertEqual(
            mock_sleep.call_count,
            10,
            "MUST call time.sleep() after each chunk to drop DB locks.",
        )
        self.assertEqual(
            QueueModel.search_count([]),
            10,
            "MUST leave 10 unprocessed records for the next trigger.",
        )
        mock_trigger.assert_called_once()  # MUST re-trigger the cron job

        # cron._trigger()

    def test_03_purge_queue_website_acl(self):
        # [@ANCHOR: COMM_test_purge_queue_base_url_sudo]

        # Tests [@ANCHOR: COMM_enqueue_urls_base_url]
        """
        Verify that the purge queue service account can successfully read
        the website_id.domain field without triggering an AccessError.
        """
        svc_uid = self.env["zero_sudo.security.utils"]._get_service_uid(
            "cloudflare.user_cloudflare_purge"
        )

        # Create a pending queue item
        queue_item = (
            self.env["cloudflare.purge.queue"]
            .with_user(svc_uid)
            .create(
                {
                    "target_item": "/acl-test",
                    "purge_type": "url",
                    "website_id": self.website.id,
                }
            )
        )

        # Read the domain via the related website_id as the service account
        # We explicitly access the domain attribute to verify read ACLs.
        domain_val = str(queue_item.with_user(svc_uid).website_id.domain)
        self.assertIsInstance(domain_val, str)

    def test_04_purge_queue_tags_processing(self):
        # Tests [@ANCHOR: COMM_cf_enqueue_tags_api]
        """Verify that tag purges in the queue are processed correctly."""
        mock_post = self.safe_patch(
            "odoo.addons.cloudflare.utils.cloudflare_api.session.post"
        )

        mock_response = MagicMock()
        mock_response.status_code = 200
        mock_post.return_value = mock_response

        QueueModel = self.env["cloudflare.purge.queue"]
        tags = ["tag1", "tag2", "tag3"]
        QueueModel.enqueue_tags(tags, website_id=self.website.id)

        self.assertEqual(QueueModel.search_count([("purge_type", "=", "tag")]), 3)

        QueueModel.process_queue()

        self.assertEqual(QueueModel.search_count([("purge_type", "=", "tag")]), 0)
        mock_post.assert_called_once()
        _, kwargs = mock_post.call_args
        self.assertEqual(kwargs["json"]["tags"], tags)

    def test_05_process_queue_optimized_exists(self):
        # [@ANCHOR: COMM_test_05_process_queue_optimized_exists]
        """Verify that process_queue uses the optimized exists() method instead of filtered()."""
        mock_post = self.safe_patch(
            "odoo.addons.cloudflare.utils.cloudflare_api.session.post"
        )
        mock_response = MagicMock()
        mock_response.status_code = 200
        mock_post.return_value = mock_response

        QueueModel = self.env["cloudflare.purge.queue"]
        QueueModel.create(
            [
                {
                    "target_item": "https://example.com/opt1",
                    "purge_type": "url",
                    "website_id": self.website.id,
                },
                {
                    "target_item": "https://example.com/opt2",
                    "purge_type": "url",
                    "website_id": self.website.id,
                },
                {
                    "target_item": "https://example.com/opt3",
                    "purge_type": "url",
                    "website_id": self.website.id,
                },
            ]
        )

        # Patch exists on the BaseModel class
        original_exists = BaseModel.exists

        call_count = [0]

        def mocked_exists(self):
            call_count[0] += 1
            return original_exists(self)

        self.safe_patch_object(BaseModel, "exists", mocked_exists)

        QueueModel.process_queue()

        # It should be called a few times (for batch_records, url_records, tag_records etc if any)
        # But if filtered(lambda r: r.exists()) is used on a recordset of 3 records,
        # exists will be called at least 3 times.
        # With batch_records.exists(), it should be called 1 time for the batch.
        # Actually let's just assert call_count[0] <= 2 for the batch.
        self.assertLess(
            call_count[0],
            3,
            "exists() was called per-record, indicating N+1 filtered pattern",
        )


@tagged("post_install", "-at_install")
class TestPurgeFailureAlerts(RealTransactionCase):
    """A purge that cannot reach Cloudflare is seen: a pager incident, no pile of unread failed rows."""

    def setUp(self):
        super().setUp()
        self.Queue = self.env["cloudflare.purge.queue"]
        self.Queue.search([], limit=100000).unlink()
        self.website = self.env["website"].create(
            {"name": "Purge Alert Site", "domain": "https://purge-alert.example"}
        )
        # Creating a website makes its default pages and menus, and in an install with other website modules every
        # one of those enqueues a purge for the new site (35 rows in the whole-system run). process_queue pages 30 rows
        # per batch and raises one alert per batch, so those rows would turn the one alert this test is about into
        # two. Start from an empty queue for this site: each test enqueues exactly the purges it asserts on.
        self.Queue.search([("website_id", "=", self.website.id)]).unlink()
        self.incidents = MagicMock()
        self.safe_patch_object(
            type(self.Queue), "_pager_incidents", lambda model: self.incidents
        )
        self.safe_patch("time.sleep")

    def _reports(self, mock_method):
        """Calls of a mocked pager method that concern this test's website (other websites' queued purges also run)."""
        return [c for c in mock_method.call_args_list
                if (c[0][0].get("website_id") if c[0] and isinstance(c[0][0], dict) else c[1].get("website_id")) == self.website.id]

    def _enqueue(self, n=2):
        self.Queue.create(
            [
                {"target_item": f"https://purge-alert.example/p{i}", "purge_type": "url",
                 "website_id": self.website.id}
                for i in range(n)
            ]
        )

    def test_01_no_credentials_drops_rows_and_pages_medium(self):
        # Tests [@ANCHOR: cloudflare:COMM_report_purge_failure]
        self._enqueue()
        self.Queue.process_queue()
        self.assertEqual(
            self.Queue.search_count([("website_id", "=", self.website.id)]), 0,
            "unsendable purges must not pile up as failed rows",
        )
        reports = self._reports(self.incidents.report_incident)
        self.assertEqual(len(reports), 1)
        vals = reports[0][0][0]
        self.assertEqual(vals["source"], "Cloudflare purge queue")
        self.assertEqual(vals["severity"], "medium")
        self.assertEqual(vals["website_id"], self.website.id)
        self.assertIn("no Cloudflare API token", vals["description"])

    def test_02_api_failure_keeps_rows_failed_and_pages_high(self):
        # Tests [@ANCHOR: cloudflare:COMM_report_purge_failure]
        self.website.write({"cloudflare_api_token": "t", "cloudflare_zone_id": "z"})
        self.safe_patch("odoo.addons.cloudflare.models.purge_queue.purge_urls", return_value=False)
        self._enqueue()
        self.Queue.process_queue()
        rows = self.Queue.search([("website_id", "=", self.website.id)])
        self.assertEqual(set(rows.mapped("state")), {"failed"})
        self.assertEqual(self._reports(self.incidents.report_incident)[0][0][0]["severity"], "high")

    def test_03_success_resolves_the_incident(self):
        # Tests [@ANCHOR: cloudflare:COMM_resolve_purge_failure]
        self.website.write({"cloudflare_api_token": "t", "cloudflare_zone_id": "z"})
        for name in ("purge_urls", "purge_tags", "purge_everything"):
            self.safe_patch(f"odoo.addons.cloudflare.models.purge_queue.{name}", return_value=True)
        self._enqueue()
        self.Queue.process_queue()
        self.assertEqual(self._reports(self.incidents.report_incident), [])
        self.assertEqual(len(self._reports(self.incidents.auto_resolve_incidents)), 1)
        self.incidents.auto_resolve_incidents.assert_any_call(
            "Cloudflare purge queue", website_id=self.website.id
        )

    def test_04_failed_rows_are_retried_once_an_hour_later_with_credentials(self):
        # Tests [@ANCHOR: cloudflare:COMM_requeue_failed_purges]
        self.website.write({"cloudflare_api_token": "t", "cloudflare_zone_id": "z"})
        rows = self.Queue.create(
            [{"target_item": f"https://purge-alert.example/f{i}", "purge_type": "url",
              "website_id": self.website.id, "state": "failed"} for i in range(3)]
        )
        self.assertEqual(self.Queue.requeue_failed(), 0, "a fresh failure is not retried yet")
        self.env.cr.execute(  # audit-ignore-sql: test back-dates write_date
            "UPDATE cloudflare_purge_queue SET write_date = now() at time zone 'utc' - interval '2 hours' WHERE id = ANY(%s)",
            (rows.ids,),
        )
        rows.invalidate_recordset()
        self.assertEqual(self.Queue.requeue_failed(), 3)
        self.assertEqual(set(rows.mapped("state")), {"pending"})

    def test_05_failed_rows_without_credentials_are_not_retried(self):
        # Tests [@ANCHOR: cloudflare:COMM_requeue_failed_purges]
        row = self.Queue.create(
            {"target_item": "https://purge-alert.example/x", "purge_type": "url",
             "website_id": self.website.id, "state": "failed"}
        )
        self.env.cr.execute(  # audit-ignore-sql: test back-dates write_date
            "UPDATE cloudflare_purge_queue SET write_date = now() at time zone 'utc' - interval '2 hours' WHERE id = %s",
            (row.id,),
        )
        row.invalidate_recordset()
        self.assertEqual(self.Queue.requeue_failed(), 0)
        self.assertEqual(row.state, "failed")

    def test_06_pager_incidents_is_none_without_pager_duty(self):
        # Tests [@ANCHOR: cloudflare:COMM_purge_pager_incidents]
        self.safe_patch_object(type(self.Queue), "_pager_incidents", lambda model: None)
        self.assertFalse(self.Queue._report_purge_failure(self.website, "x", "high"))
        self.assertFalse(self.Queue._resolve_purge_failure(self.website))


@tagged("post_install", "-at_install")
class TestPurgeResolveAsPurgeAccount(RealTransactionCase):
    """The cron runs as the purge service account; resolving the pager incident must not need that account to read contacts."""

    def test_01_resolve_posts_the_note_when_the_purge_account_is_the_default_env(self):
        # Tests [@ANCHOR: cloudflare:COMM_purge_pager_default_env]
        # Tests [@ANCHOR: cloudflare:COMM_resolve_purge_failure]
        Queue = self.env["cloudflare.purge.queue"]
        website = self.env["website"].create({"name": "Purge Resolve Site", "domain": "https://purge-resolve.example"})
        pager_uid = self.env["zero_sudo.security.utils"]._get_service_uid("pager_duty.user_pager_service_internal")
        incident = self.env["pager.incident"].with_user(pager_uid).create(
            {"name": "Purge failing", "description": "purges not reaching Cloudflare", "source": "Cloudflare purge queue", "website_id": website.id, "severity": "high"}
        )
        purge_env = self.env(user=self.env.ref("cloudflare.user_cloudflare_purge").id)
        transaction = self.env.transaction
        previous_default_env = transaction.default_env
        transaction.default_env = purge_env
        self.addCleanup(setattr, transaction, "default_env", previous_default_env)

        purge_env["cloudflare.purge.queue"]._resolve_purge_failure(website)

        self.assertEqual(incident.status, "resolved")
        self.assertIs(transaction.default_env, purge_env, "the purge account is the default env again afterwards")
