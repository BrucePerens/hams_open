# -*- coding: utf-8 -*-
# Copyright © Bruce Perens K6BP.
# SPDX-License-Identifier: AGPL-3.0-or-later
"""Deleting a user outright runs the unlink cleanup of their pages, posts and blogs (Bruce, 2026-11-16)."""
from odoo.addons.zero_sudo.tests.common import HamsTransactionCase
from odoo.tests.common import tagged


@tagged("post_install", "-at_install")
class TestUserDeleteCleanup(HamsTransactionCase):
    def _member(self, login):
        return self.env["res.users"].create({"name": login, "login": login})

    # [@ANCHOR: user_websites:test_user_delete_runs_page_and_post_cleanup]
    def test_user_delete_runs_page_and_post_cleanup(self):
        # Tests [@ANCHOR: user_websites:res_users_unlink_runs_cleanup]
        user = self._member("uw_del_ann")
        blog = self.env["blog.blog"].create({"name": "Del Ann Blog", "owner_user_id": user.id})
        post = self.env["blog.post"].create({"name": "Del Ann Post", "blog_id": blog.id, "owner_user_id": user.id})
        post_cls = type(post)
        seen = []
        post_unlink = post_cls.unlink

        def post_spy(recs):
            seen.append(("post", recs.ids))
            return post_unlink(recs)

        self.safe_patch_object(post_cls, "unlink", post_spy)
        user.unlink()
        self.assertEqual(seen, [("post", [post.id])], "the post's own unlink (cache invalidation) ran")
        self.assertFalse(post.exists())
        self.assertFalse(blog.exists())

    def test_user_delete_goes_on_when_a_cleanup_fails(self):
        # Tests [@ANCHOR: user_websites:res_users_unlink_runs_cleanup]
        user = self._member("uw_del_bob")
        blog = self.env["blog.blog"].create({"name": "Del Bob Blog", "owner_user_id": user.id})
        post = self.env["blog.post"].create({"name": "Del Bob Post", "blog_id": blog.id, "owner_user_id": user.id})

        def boom(recs):
            raise ValueError("cloudflare down")

        self.safe_patch_object(type(post), "unlink", boom)
        with self.assertLogs("odoo.addons.user_websites.models.res_users", level="ERROR") as logs:
            user.unlink()
        self.assertTrue(any("blog.post" in m for m in logs.output))
        self.assertFalse(user.exists(), "the delete was not blocked")
