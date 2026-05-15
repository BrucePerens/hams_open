# -*- coding: utf-8 -*-
from unittest.mock import patch, MagicMock
from odoo.tests.common import TransactionCase, tagged


@tagged("post_install", "-at_install")
class TestPurgeQueue(TransactionCase):
    def setUp(self):
        super().setUp()
        self.env["cloudflare.purge.queue"].search([]).unlink()
        self.website = self.env["website"].get_current_website()
        self.website.write(
            {"cloudflare_api_token": "fake_token", "cloudflare_zone_id": "fake_zone"}
        )

    @patch("odoo.addons.cloudflare.utils.cloudflare_api.requests.post")
    @patch("time.sleep")
    def test_01_bdd_queue_batching_and_rate_limiting(self, mock_sleep, mock_post):
        # [@ANCHOR: test_queue_batching_and_rate_limiting]
        # Tests [@ANCHOR: ir_cron_process_cf_purge_queue]
        # Tests [@ANCHOR: cf_process_queue_logic]
        mock_response = MagicMock()
        mock_response.status_code = 200
        mock_post.return_value = mock_response

        QueueModel = self.env["cloudflare.purge.queue"]
        vals = []
        for i in range(310):
            vals.append({
                "target_item": f"https://example.com/page-{i}",
                "purge_type": "url",
                "website_id": self.website.id,
            })
        QueueModel.create(vals)
        self.assertEqual(QueueModel.search_count([]), 310)

        cron = self.env.ref(
            "cloudflare.ir_cron_process_cf_purge_queue", raise_if_not_found=False
        )
        with patch.object(type(cron), "_trigger") as mock_trigger:
            QueueModel.process_queue()

            self.assertEqual(
                mock_post.call_count, 10, "MUST batch exactly 10 requests."
            )
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
        try:
            # We explicitly access the domain attribute to verify read ACLs.
            # We wrap it in str() to ensure it's evaluated, but discard the assignment
            # to satisfy both the test intent and flake8 static analysis (F841).
            str(queue_item.with_user(svc_uid).website_id.domain)
            self.assertTrue(True)
        except Exception as e:
            self.fail(f"Service account lacks ACLs to read website_id domain: {e}")

    @patch("odoo.addons.cloudflare.utils.cloudflare_api.requests.post")
    def test_04_purge_queue_tags_processing(self, mock_post):
        # Tests [@ANCHOR: cf_enqueue_tags_api]
        """Verify that tag purges in the queue are processed correctly."""
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
