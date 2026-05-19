# -*- coding: utf-8 -*-
from unittest.mock import patch
from odoo.tests.common import TransactionCase, tagged


@tagged("post_install", "-at_install")
class TestMultiWebsiteCloudflare(TransactionCase):

    def setUp(self):
        super().setUp()
        self.Website = self.env["website"]
        self.PurgeQueue = self.env["cloudflare.purge.queue"]

        # Create two websites with different Cloudflare credentials
        self.website_a = self.Website.create({
            "name": "Website A",
            "domain": "https://website-a.com",
            "cloudflare_api_token": "token_a",
            "cloudflare_zone_id": "zone_a",
        })
        self.website_b = self.Website.create({
            "name": "Website B",
            "domain": "https://website-b.com",
            "cloudflare_api_token": "token_b",
            "cloudflare_zone_id": "zone_b",
        })

    @patch("odoo.addons.cloudflare.models.purge_queue.purge_urls")
    def test_multi_website_purge_queue(self, mock_purge_urls):
        """Verify that the purge queue correctly isolates zones and credentials."""
        mock_purge_urls.return_value = True

        # Enqueue URLs for both websites
        self.PurgeQueue.enqueue_urls(["/page-a"], website_id=self.website_a.id)
        self.PurgeQueue.enqueue_urls(["/page-b"], website_id=self.website_b.id)

        # Process the queue
        # In the first iteration, it picks records for website_a (due to order website_id)
        # filtered(lambda r: r.website_id == first_website)
        self.PurgeQueue.process_queue()

        # Since it only processes records for the FIRST website in the batch,
        # and our batch limit (30) is larger than our 2 records,
        # it will process Website A's records, then Website B's records in the next iteration
        # of the 'while batches_processed < max_batches' loop.

        self.assertEqual(mock_purge_urls.call_count, 2)

        calls = mock_purge_urls.call_args_list
        # args = (urls, token, zone_id)

        call_a = next(c for c in calls if c[0][2] == "zone_a")
        self.assertEqual(call_a[0][1], "token_a")
        self.assertEqual(call_a[0][0], ["https://website-a.com/page-a"])

        call_b = next(c for c in calls if c[0][2] == "zone_b")
        self.assertEqual(call_b[0][1], "token_b")
        self.assertEqual(call_b[0][0], ["https://website-b.com/page-b"])

    def test_content_hook_multi_website(self):
        """Verify that editing a page linked to a specific website only enqueues for that website."""
        # Create a page for Website A
        # In Odoo 19, we must create a view first.
        view_a = self.env["ir.ui.view"].create({
            "name": "Page A View",
            "type": "qweb",
            "arch": "<div>A</div>",
            "key": "test.page_a_view",
        })
        page_a = self.env["website.page"].create({
            "is_published": True,
            "url": "/hook-a",
            "website_id": self.website_a.id,
            "view_id": view_a.id,
        })

        # Clear queue
        self.PurgeQueue.search([]).unlink()

        # Trigger write
        page_a.write({"name": "Page A Updated"})

        # Check queue
        queue_items = self.PurgeQueue.search([("target_item", "=", "https://website-a.com/hook-a")])
        self.assertEqual(len(queue_items), 1)
        self.assertEqual(queue_items.website_id.id, self.website_a.id)

        # Create a global page (no website_id)
        view_global = self.env["ir.ui.view"].create({
            "name": "Global Page View",
            "type": "qweb",
            "arch": "<div>Global</div>",
            "key": "test.page_global_view",
        })
        page_global = self.env["website.page"].create({
            "is_published": True,
            "url": "/hook-global",
            "website_id": False,
            "view_id": view_global.id,
        })

        self.PurgeQueue.search([]).unlink()
        page_global.write({"name": "Global Page Updated"})

        # Should enqueue for ALL websites that have CF credentials (A and B in our case)
        # Note: In a real test db there might be other websites too.
        queue_items = self.PurgeQueue.search([("target_item", "like", "%/hook-global")])
        websites_in_queue = queue_items.mapped("website_id")
        self.assertIn(self.website_a, websites_in_queue)
        self.assertIn(self.website_b, websites_in_queue)

    @patch("odoo.addons.cloudflare.models.ip_ban.ban_ip")
    def test_waf_ban_multi_website(self, mock_ban_ip):
        """Verify IP banning respects the website context."""
        mock_ban_ip.return_value = (True, "rule_123")

        # Ban IP on Website B
        self.env["cloudflare.waf"].ban_ip("1.2.3.4", website_id=self.website_b.id)

        # Check that ban record is linked to Website B
        ban_record = self.env["cloudflare.ip.ban"].search([("ip_address", "=", "1.2.3.4")])
        self.assertEqual(ban_record.website_id.id, self.website_b.id)

        # Verify API called with Website B credentials
        mock_ban_ip.assert_called_once()
        args = mock_ban_ip.call_args
        self.assertEqual(args[0][3], "token_b") # token
        self.assertEqual(args[0][4], "zone_b")  # zone_id
