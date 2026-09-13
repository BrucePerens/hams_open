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
        # Tests [@ANCHOR: user_websites:COMM_blog_blog_create]

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
        # Tests [@ANCHOR: user_websites:COMM_get_blog_urls]

        # Tests [@ANCHOR: user_websites:COMM_get_blog_limit]

        # Tests [@ANCHOR: user_websites:COMM_get_blog_post_limit]

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

    def test_04_portal_owner_cannot_set_website_id_to_opt_out_of_isolation(self):
        # bug-hunt (2026-09-13): website_page.py's own write()/create() already
        # had this exact class of bug found and fixed (see its own comment,
        # "website_page owner can opt their own page out of multi-website
        # tenant isolation") -- website_id was removed from THAT model's
        # allowlist because a non-admin caller could set/clear it via RPC,
        # and stock Odoo's own website_domain() (`Domain('website_id', 'in',
        # [False, *self.ids])`) treats website_id=False as "visible on every
        # website in the install." blog_blog.py's create()/write() carried
        # the identical gap. Unlike blog.post's own website_id
        # (related='blog_id.website_id', readonly=True -- genuinely
        # non-writable regardless of any allowlist), blog.blog's website_id
        # comes from stock website.multi.mixin and is a plain, independently
        # writable Many2one -- confirmed directly against
        # /usr/lib/python3/dist-packages/odoo/addons/website/models/
        # mixins.py's own WebsiteMultiMixin. The fixture blog below is
        # created via this test's own admin env (matching test_01/02/03's
        # own convention), so its own real create() call is exempt from the
        # allowlist and website_id lands as given -- this test is about the
        # WRITE-side gap specifically, isolated from create()'s own
        # (now also fixed) behavior.
        website_a = self.env["website"].get_current_website()
        blog = self.env["blog.blog"].create(
            {
                "name": "Isolation Probe Blog",
                "owner_user_id": self.user.id,
                "website_id": website_a.id,
            }
        )
        self.assertEqual(
            blog.website_id.id,
            website_a.id,
            "fixture: blog must start out bound to website_a",
        )

        self.env["blog.blog"].with_user(self.user).browse(blog.id).write(
            {"website_id": False}
        )
        blog.invalidate_recordset()

        self.assertEqual(
            blog.website_id.id,
            website_a.id,
            "[!] DIAGNOSTIC FOR AI: a portal blog owner must not be able to null "
            "website_id via a plain write() -- doing so would make the blog (and, via "
            "the related field, every post under it) render on every website in the "
            "install instead of just their own, the same multi-website-isolation-"
            "opt-out bug already found and fixed for website.page.",
        )
