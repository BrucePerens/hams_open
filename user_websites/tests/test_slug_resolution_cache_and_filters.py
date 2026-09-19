# -*- coding: utf-8 -*-
# Copyright © Bruce Perens K6BP.
# SPDX-License-Identifier: AGPL-3.0-or-later
"""Regression tests for `_get_user_id_by_slug()` and the content routing view.

The function's bug-hunt claim recorded two gaps as *latent*, on the strength
of a grep that covered `hams_open` only: a stale slug -> user-id mapping after
a rename, and a view that filtered on `active` alone. `hams_com`'s public
logbook and profile widget routes call the function on every request
(`ham_logbook/controllers/widget_api.py`, `website_logbook.py`,
`ham_profile/controllers/widget_api.py`), so both were live. These tests pin
the fixed behaviour.
"""

from odoo.tests import tagged
from odoo.addons.zero_sudo.tests.real_transaction import RealTransactionCase


@tagged("post_install", "-at_install")
class TestSlugResolutionCacheAndFilters(RealTransactionCase):
    def setUp(self):
        super().setUp()
        # Cloudflare purge hooks would otherwise leak queue records through
        # teardown, the same way every other test class in this module
        # guards against.
        self.safe_patch(
            "odoo.addons.cloudflare.models.purge_queue.CloudflarePurgeQueue.enqueue_urls",
            return_value=True,
        )
        self.safe_patch(
            "odoo.addons.cloudflare.models.purge_queue.CloudflarePurgeQueue.enqueue_tags",
            return_value=True,
        )
        self.users = self.env["res.users"]

    def _make_user(self, login, slug, **extra):
        vals = {
            "name": login,
            "login": login,
            "website_slug": slug,
            "group_ids": [
                (
                    6,
                    0,
                    [
                        self.env.ref("base.group_portal").id,
                        self.env.ref("user_websites.group_user_websites_user").id,
                    ],
                )
            ],
        }
        vals.update(extra)
        user = self.users.create(vals)
        self.env.flush_all()
        return user

    def _resolve_uncached(self, slug):
        """Resolve straight through the SQL view, skipping both cache tiers.

        `redis_bypass_cache` is `@distributed_cache()`'s own documented
        bypass. Used where the assertion is about the *view's* filters, so a
        cache hit can never make a filter test pass or fail for the wrong
        reason.
        """
        return self.users.with_context(redis_bypass_cache=True)._get_user_id_by_slug(
            slug
        )

    # Tests [@ANCHOR: user_websites:COMM_res_users_write]
    # Tests [@ANCHOR: user_websites:COMM_get_user_id_by_slug]
    def test_01_renamed_slug_is_invalidated_not_left_stale(self):
        """A slug rename must evict the cached old_slug -> old_user mapping.

        Asserted in three steps so the test cannot pass for the wrong
        reason: the entry is proven cached (it still resolves after the
        rename and flush, before invalidation runs), then proven evicted.

        `notify_model_invalidation()` defers the real eviction to a
        `cr.postcommit` callback, so the invalidation only takes effect on a
        real commit. Running the queue directly is the same effect without
        committing test data into the database -- `Cursor.commit()` itself
        does nothing more than `postcommit.run()` for this purpose.
        """
        user = self._make_user("slugcache_one", "slug-cache-original")

        self.assertEqual(
            self.users._get_user_id_by_slug("slug-cache-original"),
            user.id,
            "The slug must resolve before the rename; this call is also what "
            "populates the cache entry the rest of this test is about.",
        )

        user.write({"website_slug": "slug-cache-renamed"})
        self.env.flush_all()

        self.assertEqual(
            self.users._get_user_id_by_slug("slug-cache-original"),
            user.id,
            "Precondition: before invalidation runs, the stale entry must "
            "still be served. If this fails the cache never held the entry "
            "and the eviction assertion below would prove nothing.",
        )

        self.env.cr.postcommit.run()

        self.assertFalse(
            self.users._get_user_id_by_slug("slug-cache-original"),
            "After a rename, the old slug must stop resolving to the old "
            "user -- otherwise a different user who later claims the freed "
            "slug is shadowed by the previous owner for up to 24h.",
        )
        self.assertEqual(
            self.users._get_user_id_by_slug("slug-cache-renamed"),
            user.id,
            "The new slug must resolve to the same user.",
        )

    # Tests [@ANCHOR: user_websites:COMM_res_users_write]
    def test_02_freed_slug_resolves_to_its_new_owner(self):
        """The full exploit shape: rename, then a different user takes the
        freed slug. Visitors must reach the new owner, not the old one."""
        first = self._make_user("slugcache_first", "slug-cache-contested")
        self.assertEqual(
            self.users._get_user_id_by_slug("slug-cache-contested"), first.id
        )

        first.write({"website_slug": "slug-cache-first-moved"})
        self.env.flush_all()
        self.env.cr.postcommit.run()

        second = self._make_user("slugcache_second", "slug-cache-contested")
        self.env.cr.postcommit.run()

        self.assertEqual(
            self.users._get_user_id_by_slug("slug-cache-contested"),
            second.id,
            "The contested slug must resolve to whoever holds it now.",
        )

    # Tests [@ANCHOR: user_websites:COMM_content_routing_view_init]
    def test_03_service_account_slug_does_not_resolve(self):
        """A service account is a res.users row like any other. It must not
        be reachable through a public slug lookup."""
        svc = self._make_user("slugcache_service", "slug-cache-service")
        self.assertEqual(self._resolve_uncached("slug-cache-service"), svc.id)

        svc.write({"is_service_account": True})
        self.env.flush_all()

        self.assertFalse(
            self._resolve_uncached("slug-cache-service"),
            "A service account's slug must not resolve: the sibling public "
            "directory view already excludes them, and this view feeds an id "
            "straight into public widget routes.",
        )

    # Tests [@ANCHOR: user_websites:COMM_content_routing_view_init]
    def test_04_suspended_user_slug_does_not_resolve(self):
        """Page-serving routes 404 a suspended user, but the widget callers
        never rechecked suspension. The view must do it for them."""
        user = self._make_user("slugcache_suspended", "slug-cache-suspended")
        self.assertEqual(self._resolve_uncached("slug-cache-suspended"), user.id)

        user.write({"is_suspended_from_websites": True})
        self.env.flush_all()

        self.assertFalse(
            self._resolve_uncached("slug-cache-suspended"),
            "A suspended user's slug must not resolve, or their logbook and "
            "profile widgets stay publicly readable through hams_com's "
            "widget API while every real page of theirs 404s.",
        )

    # Tests [@ANCHOR: user_websites:COMM_content_routing_view_init]
    def test_05_archived_user_slug_does_not_resolve(self):
        """`active` was the view's only filter before this fix. Guard it so a
        later edit to the WHERE clause cannot drop it silently."""
        user = self._make_user("slugcache_archived", "slug-cache-archived")
        self.assertEqual(self._resolve_uncached("slug-cache-archived"), user.id)

        user.write({"active": False})
        self.env.flush_all()

        self.assertFalse(self._resolve_uncached("slug-cache-archived"))

    # Tests [@ANCHOR: user_websites:COMM_content_routing_view_init]
    def test_06_suspended_group_is_absent_from_the_routing_view(self):
        """The group half of the UNION has no consumer today
        (`_get_user_id_by_slug` hardcodes `res_model = 'res.users'`), so it is
        asserted against the view directly rather than through a resolver
        that would never look at it."""
        group = self.env["user.websites.group"].create(
            {"name": "Slug Cache Group", "website_slug": "slug-cache-group"}
        )
        self.env.flush_all()

        self.env.cr.execute(
            "SELECT res_id FROM user_websites_content_routing_view "
            "WHERE website_slug = %s AND res_model = 'user.websites.group'",
            ("slug-cache-group",),
        )
        self.assertEqual(
            [row[0] for row in self.env.cr.fetchall()],
            [group.id],
            "An unsuspended group must be routable.",
        )

        group.is_suspended_from_websites = True
        self.env.flush_all()

        self.env.cr.execute(
            "SELECT res_id FROM user_websites_content_routing_view "
            "WHERE website_slug = %s AND res_model = 'user.websites.group'",
            ("slug-cache-group",),
        )
        self.assertFalse(
            self.env.cr.fetchall(),
            "A suspended group must drop out of the routing view, so a "
            "future group-slug caller does not inherit the gap this fix "
            "just closed for users.",
        )

    # Tests [@ANCHOR: user_websites:COMM_res_users_write]
    def test_07_write_invalidates_only_for_routing_relevant_fields(self):
        """Every column the routing view reads must trigger an
        invalidation, and nothing else should -- an over-broad notify would
        evict the whole `res.users` cache namespace on every unrelated user
        write."""
        user = self._make_user("slugcache_notify", "slug-cache-notify")

        notify = self.safe_patch(
            "odoo.addons.user_websites.models.res_users.notify_model_invalidation"
        )

        for field, value in (
            ("website_slug", "slug-cache-notify-2"),
            ("is_suspended_from_websites", True),
            ("is_suspended_from_websites", False),
            ("is_service_account", True),
            ("active", False),
        ):
            notify.reset_mock()
            user.write({field: value})
            self.assertTrue(
                notify.called,
                f"Writing {field} changes what the content routing view "
                f"returns, so it must invalidate the res.users cache.",
            )
            notified = [call.args[1] for call in notify.call_args_list]
            self.assertIn("res.users", notified)
            # The suspension flag is also read by WebsitePage's own
            # @distributed_cache() page-id lookup, so it evicts that too --
            # and no other field may.
            self.assertEqual(
                "website.page" in notified,
                field == "is_suspended_from_websites",
                f"Writing {field} notified {notified}.",
            )

        notify.reset_mock()
        user.write({"name": "Renamed But Not Rerouted"})
        self.assertFalse(
            notify.called,
            "A write that cannot change the routing view's answer must not "
            "evict the whole res.users cache namespace.",
        )

