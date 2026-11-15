# -*- coding: utf-8 -*-
# Copyright © Bruce Perens K6BP.
# SPDX-License-Identifier: AGPL-3.0-or-later
from psycopg2 import IntegrityError

from odoo.tests.common import tagged
from odoo.tools import mute_logger
from odoo.addons.zero_sudo.tests.real_transaction import RealTransactionCase


@tagged("post_install", "-at_install")
class TestBlogNameUnique(RealTransactionCase):
    """A blog or post title is unique per owner (member or group).

    The old UNIQUE(name, owner_user_id, user_websites_group_id) never fired: one of the two
    owner columns is always NULL and NULLs are distinct. Two partial unique indexes now enforce it.
    """

    # Tests [@ANCHOR: user_websites_blog_blog_blog_blog]
    # Tests [@ANCHOR: user_websites_blog_post_blog_post]
    def setUp(self):
        super().setUp()
        tag = self.id().split(".")[-1]
        self.user = self.env["res.users"].create(
            {"name": "Uniq " + tag, "login": "uniq_" + tag, "email": tag + "@example.com", "website_slug": "uniq_" + tag}
        )
        self.group = self.env["user.websites.group"].create({"name": "Uniq " + tag, "website_slug": "ugrp_" + tag})
        self.other = self.env["res.users"].create(
            {"name": "Other " + tag, "login": "uother_" + tag, "email": "o" + tag + "@example.com", "website_slug": "uother_" + tag}
        )

    def _dup(self, model, vals):
        self.env[model].create(vals)
        with self.assertRaises(IntegrityError), mute_logger("odoo.sql_db"), self.env.cr.savepoint():
            self.env[model].create(vals)

    def test_01_user_blog_duplicate_title(self):
        self._dup("blog.blog", {"name": "Same", "owner_user_id": self.user.id})

    def test_02_group_blog_duplicate_title(self):
        self._dup("blog.blog", {"name": "Same", "user_websites_group_id": self.group.id})

    def test_03_user_post_duplicate_title(self):
        blog = self.env["blog.blog"].create({"name": "Host"})
        self._dup("blog.post", {"name": "Same", "blog_id": blog.id, "owner_user_id": self.user.id})

    def test_04_group_post_duplicate_title(self):
        blog = self.env["blog.blog"].create({"name": "Host"})
        self._dup("blog.post", {"name": "Same", "blog_id": blog.id, "user_websites_group_id": self.group.id})

    def test_05_different_owners_may_share_a_title(self):
        for model, extra in (("blog.blog", {}), ("blog.post", {"blog_id": self.env["blog.blog"].create({"name": "H"}).id})):
            for owner in ({"owner_user_id": self.user.id}, {"owner_user_id": self.other.id},
                          {"user_websites_group_id": self.group.id}):
                self.env[model].create({"name": "Shared", **extra, **owner})

    def test_06_soft_deleted_does_not_block_a_new_one(self):
        # An owner-deleted (archived) row keeps its name but must not block recreating it.
        blog = self.env["blog.blog"].create({"name": "Host"})
        for model, extra in (("blog.blog", {}), ("blog.post", {"blog_id": blog.id})):
            for owner in ({"owner_user_id": self.user.id}, {"user_websites_group_id": self.group.id}):
                vals = {"name": "Gone", **extra, **owner}
                old = self.env[model].create(vals)
                old.write({"active": False, "deleted_by_owner": "2026-01-01 00:00:00"})
                self.env[model].create(vals)  # no IntegrityError
                with self.assertRaises(IntegrityError), mute_logger("odoo.sql_db"), self.env.cr.savepoint():
                    self.env[model].create(vals)  # two live ones still collide
