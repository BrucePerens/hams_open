# This software is distributed under the terms of the Affero General Public License (AGPL-3).

# -*- coding: utf-8 -*-
"""
Real coverage for cloudflare.purge.queue.enqueue_urls_batch() -- had zero test coverage before
this file (confirmed by grepping tests/ for the method name). test_purge_queue.py's own suite
covers the consuming side (batching/rate-limiting when the queue is processed and sent to
Cloudflare), never the producing side that actually populates it.
"""
from odoo.tests.common import tagged
from odoo.addons.zero_sudo.tests.real_transaction import RealTransactionCase


@tagged("post_install", "-at_install")
class TestEnqueueUrlsBatch(RealTransactionCase):
    def setUp(self):
        super().setUp()
        self.Website = self.env["website"]
        self.PurgeQueue = self.env["cloudflare.purge.queue"]
        # Matches test_multi_website.py's own established convention: RealTransactionCase's
        # automated per-test cleanup does not reliably clear cloudflare.purge.queue between test
        # methods in this class (confirmed directly -- an exact-set assertion against a fresh
        # website's own website_id-scoped query still picked up extra entries from other test
        # methods' own runs without this explicit unlink).
        self.PurgeQueue.search([], limit=10000).unlink()
        self.website = self.Website.create(
            {"name": "Enqueue Test Site", "domain": "https://enqueue-test.example"}
        )

    def _pending_urls(self):
        return set(
            self.PurgeQueue.search(
                [("website_id", "=", self.website.id), ("state", "=", "pending")]
            ).mapped("target_item")
        )

    def test_a_relative_path_is_expanded_using_the_websites_own_domain(self):
        self.PurgeQueue.enqueue_urls_batch({self.website.id: ["/some/page"]})
        self.assertIn("https://enqueue-test.example/some/page", self._pending_urls())

    def test_an_already_absolute_url_is_stored_as_is_not_double_prefixed(self):
        self.PurgeQueue.enqueue_urls_batch(
            {self.website.id: ["https://other-cdn.example/asset.js"]}
        )
        self.assertIn("https://other-cdn.example/asset.js", self._pending_urls())

    def test_the_same_url_is_never_enqueued_twice_while_still_pending(self):
        self.PurgeQueue.enqueue_urls_batch({self.website.id: ["/dup"]})
        self.PurgeQueue.enqueue_urls_batch({self.website.id: ["/dup"]})
        count = self.PurgeQueue.search_count(
            [
                ("website_id", "=", self.website.id),
                ("target_item", "=", "https://enqueue-test.example/dup"),
                ("state", "=", "pending"),
            ]
        )
        self.assertEqual(count, 1)

    def test_duplicate_urls_within_the_same_call_are_also_deduplicated(self):
        self.PurgeQueue.enqueue_urls_batch({self.website.id: ["/same", "/same", "/same"]})
        count = self.PurgeQueue.search_count(
            [
                ("website_id", "=", self.website.id),
                ("target_item", "=", "https://enqueue-test.example/same"),
            ]
        )
        self.assertEqual(count, 1)

    def test_an_empty_purge_map_is_a_no_op(self):
        before = self.PurgeQueue.search_count([])
        self.PurgeQueue.enqueue_urls_batch({})
        self.assertEqual(self.PurgeQueue.search_count([]), before)

    def test_falsy_urls_in_the_list_are_skipped(self):
        # assertIn, not an exact-set match: creating self.website in setUp() has its own,
        # unrelated side effect of queuing a purge for the new site's auto-created homepage (a
        # real, separate cloudflare.purge.mixin consumer reacting to Odoo's own default-content
        # creation on a new website, confirmed directly by comparing pending entries before vs.
        # after website creation) -- this test only cares whether the falsy entries in ITS OWN
        # input were skipped, not what the full pending set looks like.
        before = self._pending_urls()
        self.PurgeQueue.enqueue_urls_batch({self.website.id: ["/real", "", None]})
        after = self._pending_urls()
        self.assertEqual(after - before, {"https://enqueue-test.example/real"})
