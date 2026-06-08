# -*- coding: utf-8 -*-
from odoo.tests.common import tagged
from odoo.addons.zero_sudo.tests.common import HamsTransactionCase


@tagged("post_install", "-at_install")
class TestMultiWebsiteCloudflare(HamsTransactionCase):

    def setUp(self):
        super().setUp()
        self.Website = self.env["website"]
        self.PurgeQueue = self.env["cloudflare.purge.queue"]

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

    def test_multi_website_purge_queue(self):
        """Verify that the purge queue correctly isolates zones and credentials."""
        mock_purge_urls = self.safe_patch(
            "odoo.addons.cloudflare.models.purge_queue.purge_urls"
        )
        mock_purge_urls.return_value = True
        self.safe_patch(
            "odoo.addons.cloudflare.models.purge_queue.purge_tags", return_value=True
        )
        # Patch exactly where it is imported/used, not where it is defined.
        self.safe_patch(
            "odoo.addons.cloudflare.models.purge_queue.purge_everything", return_value=True
        )

        # Enqueue URLs for both websites
        self.PurgeQueue.enqueue_urls(["/page-a"], website_id=self.website_a.id)
        self.PurgeQueue.enqueue_urls(["/page-b"], website_id=self.website_b.id)

        # Process the queue
        self.PurgeQueue.process_queue()

        self.assertEqual(mock_purge_urls.call_count, 2)

        calls = mock_purge_urls.call_args_list

        call_a = next(c for c in calls if c[0][2] == "zone_a")
        self.assertEqual(call_a[0][1], "token_a")
        # Enqueue pushes root URL alongside specific paths automatically for structural cache invalidation
        self.assertIn("https://website-a.com/", call_a[0][0])
        self.assertIn("https://website-a.com/page-a", call_a[0][0])

        call_b = next(c for c in calls if c[0][2] == "zone_b")
        self.assertEqual(call_b[0][1], "token_b")
        self.assertIn("https://website-b.com/", call_b[0][0])
        self.assertIn("https://website-b.com/page-b", call_b[0][0])

    def test_content_hook_multi_website(self):
        """Verify that editing a page linked to a specific website only enqueues for that website."""
        view_a = self.env["ir.ui.view"].create(
            {
                "name": "Page A View",
                "type": "qweb",
                "arch_db": "<div>A</div>",
                "key": "test.page_a_view",
            }
        )
        page_a = self.env["website.page"].create(
            {
                "is_published": True,
                "url": "/hook-a",
                "website_id": self.website_a.id,
                "view_id": view_a.id,
            }
        )

        # Clear queue
        self.PurgeQueue.search([]).unlink()

        # Trigger write
        page_a.write({"name": "Page A Updated"})

        # Check queue
        queue_items = self.PurgeQueue.search(
            [("target_item", "=", "https://website-a.com/hook-a")]
        )
        self.assertEqual(len(queue_items), 1)
        self.assertEqual(queue_items.website_id.id, self.website_a.id)

        # Create a global page (no website_id)
        view_global = self.env["ir.ui.view"].create(
            {
                "name": "Global Page View",
                "type": "qweb",
                "arch_db": "<div>Global</div>",
                "key": "test.page_global_view",
            }
        )
        page_global = self.env["website.page"].create(
            {
                "is_published": True,
                "url": "/hook-global",
                "website_id": False,
                "view_id": view_global.id,
            }
        )

        self.PurgeQueue.search([]).unlink()
        page_global.write({"name": "Global Page Updated"})

        # Should enqueue for ALL websites that have CF credentials
        queue_items = self.PurgeQueue.search([("target_item", "like", "%/hook-global")])
        websites_in_queue = queue_items.mapped("website_id")
        self.assertIn(self.website_a, websites_in_queue)
        self.assertIn(self.website_b, websites_in_queue)

    def test_waf_ban_multi_website(self):
        """Verify IP banning respects the website context."""
        mock_ban_ip = self.safe_patch("odoo.addons.cloudflare.models.ip_ban.ban_ip")
        mock_ban_ip.return_value = (True, "rule_123")

        # Ban IP on Website B
        self.env["cloudflare.waf"].ban_ip("1.2.3.4", website_id=self.website_b.id)

        # Check that ban record is linked to Website B
        ban_record = self.env["cloudflare.ip.ban"].search(
            [("ip_address", "=", "1.2.3.4")]
        )
        self.assertEqual(ban_record.website_id.id, self.website_b.id)

        # Verify API called with Website B credentials
        mock_ban_ip.assert_called_once()
        args = mock_ban_ip.call_args
        self.assertEqual(args[0][3], "token_b")  # token
        self.assertEqual(args[0][4], "zone_b")  # zone_id
