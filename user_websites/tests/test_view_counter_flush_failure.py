# Copyright © Bruce Perens K6BP.
# SPDX-License-Identifier: AGPL-3.0-or-later
"""Real transaction: a failed SQL write in the view-counter flush must not lose the Redis counts."""
import odoo
from odoo.addons.user_websites.models.website_page import redis_client
from odoo.addons.zero_sudo.tests.real_transaction import RealTransactionCase
from odoo.tests.common import tagged
from odoo.tools import mute_logger

SVC = "user_websites.user_websites_service_account"


@tagged("post_install", "-at_install")
class TestViewCounterFlushFailure(RealTransactionCase):
    # [@ANCHOR: user_websites:test_view_counter_flush_failure]
    def _page(self):
        page = self.env["website.page"].create({
            "name": "Flush Failure", "website_id": 1, "url": "/flush-failure", "type": "qweb",
            "arch": "<div></div>", "key": "website.flush_failure_page",
        })
        self.env.cr.commit()
        self.addCleanup(self._delete, page.id)
        key = f"views:{self.env.cr.dbname}:page:{page.id}"
        self.addCleanup(redis_client.delete, key)
        return page, key

    def _delete(self, page_id):
        with self.registry.cursor() as cr:
            cr.execute("DELETE FROM website_page WHERE id = %s", (page_id,))

    def _flush(self, page_id, lock):
        """Run the flush; with lock, another connection holds the page row so the write times out (a real failure)."""
        svc_uid = self.env["zero_sudo.security.utils"]._get_service_uid(SVC)
        with self.registry.cursor() as holder:
            if lock:
                holder.execute("SELECT id FROM website_page WHERE id = %s FOR UPDATE", (page_id,))
            with self.registry.cursor() as cr, mute_logger("odoo.addons.user_websites.models.website_page"):
                cr.execute("SET LOCAL lock_timeout = '200ms'")
                odoo.api.Environment(cr, svc_uid, {})["website.page"]._flush_redis_view_counters()

    def _count(self, page_id):
        with self.registry.cursor() as cr:
            cr.execute("SELECT view_count FROM website_page WHERE id = %s", (page_id,))
            return cr.fetchone()[0]

    def test_failed_sql_write_keeps_the_redis_counts(self):
        # Tests [@ANCHOR: user_websites:website_page_flush_redis_view_counters]
        page, key = self._page()
        before = self._count(page.id)
        redis_client.set(key, 7)
        self._flush(page.id, lock=True)
        self.assertEqual(redis_client.get(key), b"7", "the counts survive a failed write")
        self.assertEqual(self._count(page.id), before)
        self._flush(page.id, lock=False)
        self.assertEqual(self._count(page.id), before + 7)
        self.assertEqual(redis_client.exists(key), 0)
