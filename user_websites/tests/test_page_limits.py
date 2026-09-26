# -*- coding: utf-8 -*-
# Copyright © Bruce Perens K6BP.
# SPDX-License-Identifier: AGPL-3.0-or-later
import json

from odoo.tests.common import tagged
from odoo.addons.zero_sudo.tests.real_transaction import RealTransactionCase
from odoo.exceptions import ValidationError


@tagged("post_install", "-at_install")
class TestPageLimits(RealTransactionCase):

    def setUp(self):
        super(TestPageLimits, self).setUp()

        # User with a strict personal limit of 2 pages
        self.user_limited = self.env["res.users"].create(
            {
                "name": "Limited User",
                "login": "limiteduser",
                "email": "limited@example.com",
                "website_slug": "limiteduser",
                "website_page_limit": 2,
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
        )

        # User with 0 limit, relying on the global system parameter
        self.user_global = self.env["res.users"].create(
            {
                "name": "Global Limit User",
                "login": "globaluser",
                "email": "global@example.com",
                "website_slug": "globaluser",
                "website_page_limit": 0,
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
        )

        # Set the global fallback limit to 3 for testing purposes
        self.env["ir.config_parameter"].set_param(
            "user_websites.global_website_page_limit", "3"
        )

    def test_01_user_specific_limit_enforcement(self):
        # Tests [@ANCHOR: user_websites:COMM_res_users_create]

        # Tests [@ANCHOR: user_websites:COMM_get_page_limit]

        # Tests [@ANCHOR: user_websites:COMM_page_invalidate_cloudflare_cache]

        # [@ANCHOR: test_page_quota_limit]

        # Tests [@ANCHOR: website_page_quota_check]

        # Tests [@ANCHOR: test_page_limits]
        """
        Verify that a user with a specific limit can create pages up to that limit,
        and is blocked upon exceeding it.
        """
        # Create pages up to the limit (2 pages)
        for i in range(2):
            self.env["website.page"].create(
                {
                    "url": f"/limiteduser/page-{i}",
                    "name": f"Page {i}",
                    "type": "qweb",
                    "owner_user_id": self.user_limited.id,
                }
            )

        # Attempt to create one more page past the limit
        with self.assertRaises(
            ValidationError,
            msg="User should not be able to exceed their personal page limit.",
        ):
            self.env["website.page"].create(
                {
                    "url": "/limiteduser/page-excess",
                    "name": "Excess Page",
                    "type": "qweb",
                    "owner_user_id": self.user_limited.id,
                }
            )
            self.env.flush_all()

    def test_02_global_limit_fallback_enforcement(self):
        """
        Verify that if a user's specific limit is 0, the system correctly falls back
        to the global ir.config_parameter limit.
        """
        # Create pages up to the global limit (3 pages)
        for i in range(3):
            self.env["website.page"].create(
                {
                    "url": f"/globaluser/page-{i}",
                    "name": f"Global Page {i}",
                    "type": "qweb",
                    "owner_user_id": self.user_global.id,
                }
            )

        # Attempt to create one more page past the global limit
        with self.assertRaises(
            ValidationError, msg="User should be blocked by the global limit fallback."
        ):
            self.env["website.page"].create(
                {
                    "url": "/globaluser/page-excess",
                    "name": "Excess Global Page",
                    "type": "qweb",
                    "owner_user_id": self.user_global.id,
                }
            )
            self.env.flush_all()

    def test_03_group_limit_enforcement(self):
        """
        Verify that a group's page creations are evaluated against the global limit.
        """
        test_group = self.env["user.websites.group"].create(
            {"name": "Limit Group", "website_slug": "limitgroup"}
        )

        # Global limit was set to 3 in setUp
        for i in range(3):
            self.env["website.page"].create(
                {
                    "url": f"/limitgroup/page-{i}",
                    "name": f"Group Page {i}",
                    "type": "qweb",
                    "user_websites_group_id": test_group.id,
                }
            )

        with self.assertRaises(
            ValidationError, msg="Group should be blocked by the global limit fallback."
        ):
            self.env["website.page"].create(
                {
                    "url": "/limitgroup/page-excess",
                    "name": "Excess Group Page",
                    "type": "qweb",
                    "user_websites_group_id": test_group.id,
                }
            )
            self.env.flush_all()

    def test_04_get_page_id_by_url_resolves_a_published_page(self):
        # Tests [@ANCHOR: user_websites:COMM_get_page_id_by_url]
        """_get_page_id_by_url() had zero test coverage -- confirmed
        (grepped both repos) that nothing currently calls it either.
        Tested directly here as a genuinely reachable unit regardless of
        caller wiring, and flagged as dead/unwired code for a deliberate
        follow-up decision (wire it up, or remove it), the same as
        _get_user_id_by_slug()'s identical situation."""
        page = self.env["website.page"].create(
            {
                "url": "/direct-url-lookup-test",
                "name": "Direct URL Lookup Test",
                "type": "qweb",
                "owner_user_id": self.user_limited.id,
                "website_published": True,
            }
        )
        self.env.flush_all()

        resolved_id = self.env["website.page"]._get_page_id_by_url(
            "/direct-url-lookup-test", False
        )
        self.assertEqual(resolved_id, page.id)

        self.assertFalse(
            self.env["website.page"]._get_page_id_by_url(
                "/no-such-url-at-all", False
            ),
            "An unknown URL must resolve to False, not raise.",
        )

        # A suspended owner's page must not resolve either, even though
        # the row itself still exists and is still marked published. Must
        # go through the real action (not a raw field write) since that is
        # the only path production code ever takes, and is what actually
        # invalidates _get_page_id_by_url()'s distributed cache entry --
        # a raw field write bypasses that invalidation entirely.
        #
        # notify_model_invalidation() registers its actual cache-clearing
        # work as a cr.postcommit callback (to close the race window
        # between an intervening read and the write finishing) -- that
        # callback only runs from a real cr.commit(), not flush_all(),
        # so a real commit is required here for the invalidation to
        # actually take effect before the next lookup below.
        self.user_limited.action_suspend_user_websites()
        self.env.cr.commit()
        self.assertFalse(
            self.env["website.page"]._get_page_id_by_url(
                "/direct-url-lookup-test", False
            ),
            "A suspended owner's page must not resolve via direct URL lookup.",
        )

    def test_05_direct_suspension_write_evicts_the_cached_page_id(self):
        # Tests [@ANCHOR: user_websites:COMM_get_page_id_by_url]
        # Tests [@ANCHOR: user_websites:COMM_res_users_write]
        """_get_page_id_by_url() checks the owner's suspension flag only when
        it POPULATES its cache entry. A direct `is_suspended_from_websites`
        write (not the suspend/pardon actions, which evict on their own) used
        to leave the already-resolved page id served by that worker."""
        page = self.env["website.page"].create(
            {
                "url": "/direct-suspend-write-user-test",
                "name": "Direct Suspend Write Test",
                "type": "qweb",
                "owner_user_id": self.user_limited.id,
                "website_published": True,
            }
        )
        self.env.flush_all()
        self.env.cr.commit()
        self.assertEqual(
            self.env["website.page"]._get_page_id_by_url(
                "/direct-suspend-write-user-test", False
            ),
            page.id,
            "Precondition: the page resolves (and is now cached).",
        )

        self.user_limited.write({"is_suspended_from_websites": True})
        # The eviction runs as a postcommit callback, so a real commit is needed.
        self.env.cr.commit()
        self.assertFalse(
            self.env["website.page"]._get_page_id_by_url(
                "/direct-suspend-write-user-test", False
            ),
            "A directly-suspended owner's page must stop resolving, not stay "
            "served from the cache entry populated before the suspension.",
        )

    def test_06_direct_group_suspension_write_evicts_the_cached_page_id(self):
        # Tests [@ANCHOR: user_websites:COMM_get_page_id_by_url]
        """Same as test_05, for a group-owned page and the group's own flag."""
        group = self.env["user.websites.group"].create(
            {"name": "Direct Suspend Group", "website_slug": "directsuspendgrp"}
        )
        page = self.env["website.page"].create(
            {
                "url": "/direct-suspend-write-group-test",
                "name": "Direct Suspend Write Group Test",
                "type": "qweb",
                "user_websites_group_id": group.id,
                "website_published": True,
            }
        )
        self.env.flush_all()
        self.env.cr.commit()
        self.assertEqual(
            self.env["website.page"]._get_page_id_by_url(
                "/direct-suspend-write-group-test", False
            ),
            page.id,
            "Precondition: the page resolves (and is now cached).",
        )

        group.write({"is_suspended_from_websites": True})
        self.env.cr.commit()
        self.assertFalse(
            self.env["website.page"]._get_page_id_by_url(
                "/direct-suspend-write-group-test", False
            ),
            "A directly-suspended group's page must stop resolving.",
        )

    def _count_cache_signals(self, model_name, action):
        """Runs `action` and returns how many `distributed_cache_invalidation`
        pg_notify calls naming `model_name` it issued on this cursor."""
        cr = self.env.cr
        original_execute = cr.execute
        signals = []

        def spy_execute(query, params=None, log_exceptions=True):
            if query == "SELECT pg_notify(%s, %s)":
                channel, payload = params
                if channel == "distributed_cache_invalidation":
                    signals.append(json.loads(payload).get("model"))
            return original_execute(query, params, log_exceptions)

        cr.execute = spy_execute
        try:
            action()
        finally:
            cr.execute = original_execute
        return signals.count(model_name)

    def test_07_page_write_and_unlink_signal_cache_invalidation_once(self):
        # Tests [@ANCHOR: user_websites:COMM_website_page_unlink]
        """write() and unlink() used to signal the same invalidation three
        times (the zero_sudo helper, which already delegates to
        notify_model_invalidation(), then a second direct
        notify_model_invalidation(), then a hand-inlined pg_notify).
        Exactly one is enough and exactly one is required."""
        page = self.env["website.page"].create(
            {
                "url": "/single-signal-test",
                "name": "Single Signal Test",
                "type": "qweb",
                "owner_user_id": self.user_limited.id,
                "website_published": False,
            }
        )
        self.env.flush_all()

        self.assertEqual(
            self._count_cache_signals(
                "website.page",
                lambda: page.write({"url": "/single-signal-test-renamed"}),
            ),
            1,
            "write() must signal website.page cache invalidation exactly once.",
        )
        self.assertEqual(
            self._count_cache_signals("website.page", page.unlink),
            1,
            "unlink() must signal website.page cache invalidation exactly once.",
        )

    def test_08_page_website_published_toggle_signals_cache_invalidation_once(self):
        # Tests [@ANCHOR: user_websites:website_page_write_reentrancy_guard]
        """A website_published toggle still signalled cache invalidation twice after
        test_07 above closed the triplicate case: website_published is a compute/inverse
        field (stock Odoo), and its own inverse method assigns is_published on the same
        recordset, which re-enters write() recursively for the same logical toggle --
        once for vals={'website_published': ...} and again for vals={'is_published': ...}.
        Exactly one signal is required either way."""
        page = self.env["website.page"].create(
            {
                "url": "/publish-toggle-signal-test",
                "name": "Publish Toggle Signal Test",
                "type": "qweb",
                "owner_user_id": self.user_limited.id,
                "website_published": False,
            }
        )
        self.env.flush_all()

        self.assertEqual(
            self._count_cache_signals(
                "website.page",
                lambda: page.write({"website_published": True}),
            ),
            1,
            "Toggling website_published must signal website.page cache invalidation "
            "exactly once, not once per recursive is_published re-entry.",
        )
