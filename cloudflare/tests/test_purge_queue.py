# This software is distributed under the terms of the Affero General Public License (AGPL-3).

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
        # [@ANCHOR: test_queue_batching_and_rate_limiting]
        # Tests [@ANCHOR: ir_cron_process_cf_purge_queue]
        # Tests [@ANCHOR: cf_process_queue_logic]
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

        cron._trigger()

    def test_03_purge_queue_website_acl(self):
        # [@ANCHOR: test_purge_queue_base_url_sudo]
        # Tests [@ANCHOR: enqueue_urls_base_url]
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
        # Tests [@ANCHOR: cf_enqueue_tags_api]
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
