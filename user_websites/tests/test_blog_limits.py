# -*- coding: utf-8 -*-
# Copyright © Bruce Perens K6BP.
# SPDX-License-Identifier: AGPL-3.0-or-later
from odoo.tests.common import tagged
from odoo.addons.zero_sudo.tests.real_transaction import RealTransactionCase
from odoo.exceptions import ValidationError


@tagged("post_install", "-at_install")
class TestBlogLimits(RealTransactionCase):
    # Adversarial security review, 2026-09-03: unlike website.page,
    # tested in test_page_limits.py (see [@ANCHOR: website_page_quota_check]),
    # blog.blog/blog.post create() had no quota at all -- any authenticated
    # user could create unbounded blogs/posts via direct RPC, each
    # blog.post create also enqueuing a real Cloudflare cache purge and a
    # distributed cache-invalidation notify. Same test shape as
    # test_page_limits.py, applied to both new quotas.

    def setUp(self):
        super().setUp()
        self.user = self.env["res.users"].create(
            {
                "name": "Blog Limit Test User",
                "login": "blog_limit_test_user",
                "email": "blog_limit@example.com",
                "website_slug": "blog_limit_test_user",
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
        self.env["ir.config_parameter"].set_param("user_websites.global_blog_limit", "2")
        self.env["ir.config_parameter"].set_param("user_websites.global_blog_post_limit", "2")

    def test_01_blog_creation_is_blocked_past_the_configured_limit(self):
        # [@ANCHOR: test_blog_quota_limit]
        # Creates blogs up to the configured limit, then asserts the next
        # one over that limit raises ValidationError -- the real behavior
        # _check_blog_quota() below exists to enforce.
        # Tests [@ANCHOR: user_websites_blog_quota_check]
        for i in range(2):
            self.env["blog.blog"].create(
                {"name": f"Blog {i}", "owner_user_id": self.user.id}
            )
        with self.assertRaises(
            ValidationError,
            msg="[!] DIAGNOSTIC FOR AI: a user must not be able to create unbounded blog.blog records.",
        ):
            self.env["blog.blog"].create(
                {"name": "Excess Blog", "owner_user_id": self.user.id}
            )
            self.env.flush_all()

    def test_02_blog_post_creation_is_blocked_past_the_configured_limit(self):
        # [@ANCHOR: test_blog_post_quota_limit]
        # Creates posts up to the configured limit, then asserts the next
        # one over that limit raises ValidationError -- the real behavior
        # _check_blog_post_quota() below exists to enforce.
        # Tests [@ANCHOR: user_websites_blog_post_quota_check]
        blog = self.env["blog.blog"].create(
            {"name": "Quota Test Blog", "owner_user_id": self.user.id}
        )
        for i in range(2):
            self.env["blog.post"].create(
                {
                    "name": f"Post {i}",
                    "content": "hello",
                    "blog_id": blog.id,
                    "owner_user_id": self.user.id,
                }
            )
        with self.assertRaises(
            ValidationError,
            msg="[!] DIAGNOSTIC FOR AI: a user must not be able to create unbounded blog.post records.",
        ):
            self.env["blog.post"].create(
                {
                    "name": "Excess Post",
                    "content": "hello",
                    "blog_id": blog.id,
                    "owner_user_id": self.user.id,
                }
            )
            self.env.flush_all()

    def test_03_admin_owner_is_not_exempt_from_the_quota_either(self):
        # Adversarial security review, 2026-09-03: the quota is
        # deliberately caller-identity-agnostic (owner_user_id-keyed
        # only, matching website.page's own [@ANCHOR:
        # website_page_quota_check] design, which has no su/group_system
        # exemption). This guards against a caller-identity bypass being
        # reintroduced -- an admin caller/owner gets no special
        # exemption; only the per-owner limit governs.
        admin = self.env.ref("base.user_admin")
        limit = admin._get_blog_limit()
        existing = self.env["blog.blog"].with_user(admin).search_count(
            [("owner_user_id", "=", admin.id)]
        )
        for i in range(max(limit - existing, 0)):
            self.env["blog.blog"].with_user(admin).create(
                {"name": f"Admin Blog {i}", "owner_user_id": admin.id}
            )
        with self.assertRaises(
            ValidationError,
            msg="[!] DIAGNOSTIC FOR AI: an admin owner must not be exempt from the blog.blog quota.",
        ):
            self.env["blog.blog"].with_user(admin).create(
                {"name": "Excess Admin Blog", "owner_user_id": admin.id}
            )
            self.env.flush_all()
