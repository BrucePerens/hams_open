# This software is distributed under the terms of the Affero General Public License (AGPL-3).

# -*- coding: utf-8 -*-
from odoo.tests.common import tagged
from odoo.addons.zero_sudo.tests.real_transaction import RealTransactionCase


@tagged("post_install", "-at_install")
class TestPurgeEverything(RealTransactionCase):

    def setUp(self):
        super().setUp()
        self.Website = self.env["website"]
        self.PurgeQueue = self.env["cloudflare.purge.queue"]

        # Clear queue
        self.PurgeQueue.search([], limit=10000).unlink()

        # Create two websites with different Cloudflare credentials
        self.website_a = self.Website.create(
            {
                "name": "Website A",
                "domain": "https://website-a.com",
                "cloudflare_api_token": "token_a",
                "cloudflare_zone_id": "zone_a",
            }
        )
        self.website_b = self.Website.create(
            {
                "name": "Website B",
                "domain": "https://website-b.com",
                "cloudflare_api_token": "token_b",
                "cloudflare_zone_id": "zone_b",
            }
        )

    def test_purge_everything_logic(self):
        # Tests [@ANCHOR: cf_enqueue_everything]
        """Verify that enqueue_everything correctly wipes other pending records for the same website."""
        mock_purge_everything = self.safe_patch(
            "odoo.addons.cloudflare.models.purge_queue.purge_everything"
        )
        mock_purge_everything.return_value = True
        self.safe_patch(
            "odoo.addons.cloudflare.models.purge_queue.purge_urls", return_value=True
        )
        self.safe_patch(
            "odoo.addons.cloudflare.models.purge_queue.purge_tags", return_value=True
        )

        # 1. Enqueue some URLs and Tags for Website A
        self.PurgeQueue.enqueue_urls(["/a1", "/a2"], website_id=self.website_a.id)
        self.PurgeQueue.enqueue_tags(["atag1"], website_id=self.website_a.id)

        # 2. Enqueue some URLs for Website B
        self.PurgeQueue.enqueue_urls(["/b1"], website_id=self.website_b.id)

        # enqueue_urls(website_a) adds: /a1, /, /a2, /
        # website_a has 3 unique target items: /a1, /a2, / but 4 records were created.
        # enqueue_tags adds: atag1
        # Total Website A: 5 records initially.
        self.PurgeQueue.enqueue_everything(website_ids=self.website_a.ids)
        self.PurgeQueue.process_queue()

        # Website A should have called purge_everything and all its records should be gone
        # Using any_order=True because the exact sequence of calls depends on queue ordering
        mock_purge_everything.assert_any_call("token_a", "zone_a")
        self.assertEqual(
            self.PurgeQueue.search_count([("website_id", "=", self.website_a.id)]), 0
        )

        # Website B records should also be processed and unlinked
        self.assertEqual(
            self.PurgeQueue.search_count([("website_id", "=", self.website_b.id)]), 0
        )

    def test_purge_everything_multi_website_resilience(self):
        """Verify that one website failing doesn't stop others."""
        mock_purge_everything = self.safe_patch(
            "odoo.addons.cloudflare.models.purge_queue.purge_everything"
        )
        self.safe_patch(
            "odoo.addons.cloudflare.models.purge_queue.purge_urls", return_value=True
        )
        self.safe_patch(
            "odoo.addons.cloudflare.models.purge_queue.purge_tags", return_value=True
        )

        # Website A fails
        def side_effect(token, zone):
            if zone == "zone_a":
                return False
            return True

        mock_purge_everything.side_effect = side_effect

        self.PurgeQueue.enqueue_everything(website_ids=self.website_a.ids)
        self.PurgeQueue.enqueue_everything(website_ids=self.website_b.ids)

        self.PurgeQueue.process_queue()

        # Website A should be failed
        recs_a = self.PurgeQueue.search(
            [("website_id", "=", self.website_a.id)], limit=100
        )
        self.assertTrue(all(r.state == "failed" for r in recs_a))

        # Website B should be GONE (success)
        self.assertEqual(
            self.PurgeQueue.search_count([("website_id", "=", self.website_b.id)]), 0
        )
