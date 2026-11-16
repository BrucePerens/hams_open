# Copyright © Bruce Perens K6BP.
# SPDX-License-Identifier: AGPL-3.0-or-later

# -*- coding: utf-8 -*-
from cryptography.fernet import Fernet
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
        
        fernet_key = Fernet.generate_key()
        mock_fernet = self.safe_patch("odoo.addons.cloudflare.models.website.WebsiteCloudflare._get_fernet")
        mock_fernet.return_value = Fernet(fernet_key)
        
        self.website_a.write({
            "cloudflare_api_token": "token_a",
            "cloudflare_zone_id": "zone_a",
        })
        self.website_b.write({
            "cloudflare_api_token": "token_b",
            "cloudflare_zone_id": "zone_b",
        })

    def test_purge_everything_logic(self):
        # [@ANCHOR: COMM_test_purge_everything_logic]
        # spacing
        # Tests [@ANCHOR: COMM_cf_enqueue_everything]
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

    def test_purge_queued_during_the_wipe_is_kept(self):
        # Tests [@ANCHOR: COMM_cf_process_queue_logic]
        """A purge enqueued while the purge-everything call is in flight is not covered by it."""
        self.safe_patch("odoo.addons.cloudflare.models.purge_queue.purge_urls", return_value=True)
        purge_tags = self.safe_patch(
            "odoo.addons.cloudflare.models.purge_queue.purge_tags", return_value=True
        )

        def wipe(token, zone):
            # setUp's credential writes leave a purge-everything row for website B too; only A's wipe
            # is the one this test races against.
            if token == "token_a":
                self.PurgeQueue.enqueue_tags(["late"], website_id=self.website_a.id)
            return True

        self.safe_patch(
            "odoo.addons.cloudflare.models.purge_queue.purge_everything", side_effect=wipe
        )
        self.PurgeQueue.enqueue_urls(["/early"], website_id=self.website_a.id)
        self.PurgeQueue.enqueue_everything(website_ids=self.website_a.ids)
        self.PurgeQueue.process_queue()
        # The later batch of the same run sent the late tag; the old blanket DELETE dropped it unsent.
        purge_tags.assert_called_once_with(["late"], "token_a", "zone_a")

    def test_purge_everything_multi_website_resilience(self):
        # [@ANCHOR: COMM_test_purge_everything_multi_website_resilience]
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

    def test_a_website_in_a_shared_zone_purges_its_hosts_not_the_zone(self):
        # Tests [@ANCHOR: COMM_cf_process_queue_logic]
        # Tests [@ANCHOR: cloudflare:COMM_website_purge_hosts]
        # [@ANCHOR: cloudflare:COMM_test_website_purge_hosts]
        """With cloudflare_purge_hosts set, "everything" is a purge of those host names and never of the zone."""
        self.website_a.write({"cloudflare_purge_hosts": "Hams.com, www.hams.com\nhams.com\n"})
        self.assertEqual(self.website_a._cloudflare_purge_hosts(), ["hams.com", "www.hams.com"])
        self.assertEqual(self.website_b._cloudflare_purge_hosts(), [])
        everything = self.safe_patch("odoo.addons.cloudflare.models.purge_queue.purge_everything", return_value=True)
        hosts = self.safe_patch("odoo.addons.cloudflare.models.purge_queue.purge_hosts", return_value=True)
        self.safe_patch("odoo.addons.cloudflare.models.purge_queue.purge_urls", return_value=True)
        self.safe_patch("odoo.addons.cloudflare.models.purge_queue.purge_tags", return_value=True)
        self.PurgeQueue.search([]).unlink()
        self.PurgeQueue.enqueue_everything(website_ids=self.website_a.ids)
        self.PurgeQueue.process_queue()
        hosts.assert_called_once_with(["hams.com", "www.hams.com"], "token_a", "zone_a")
        everything.assert_not_called()
        self.assertEqual(self.PurgeQueue.search_count([("website_id", "=", self.website_a.id)]), 0)
        # A failed host purge is a failed purge like any other.
        hosts.return_value = False
        self.PurgeQueue.enqueue_everything(website_ids=self.website_a.ids)
        self.PurgeQueue.process_queue()
        self.assertEqual(self.PurgeQueue.search([("website_id", "=", self.website_a.id)]).mapped("state"), ["failed"])

    def test_the_manual_purge_of_a_shared_zone_purges_its_hosts(self):
        # Tests [@ANCHOR: cloudflare:COMM_purge_wizard_action_purge]
        self.website_a.write({"cloudflare_purge_hosts": "hams.com"})
        hosts = self.safe_patch("odoo.addons.cloudflare.models.purge_wizard.purge_hosts", return_value=True)
        everything = self.safe_patch("odoo.addons.cloudflare.models.purge_wizard.purge_everything", return_value=True)
        wizard = self.env["cloudflare.purge.wizard"].create({"website_id": self.website_a.id, "purge_type": "everything"})
        self.assertEqual(wizard.action_purge()["tag"], "display_notification")
        hosts.assert_called_once_with(["hams.com"], "token_a", "zone_a")
        everything.assert_not_called()
        self.website_a.write({"cloudflare_purge_hosts": False})
        wizard.action_purge()
        everything.assert_called_once_with("token_a", "zone_a")
