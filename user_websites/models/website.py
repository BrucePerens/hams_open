# SPDX-License-Identifier: AGPL-3.0-or-later
# -*- coding: utf-8 -*-
# Copyright © Bruce Perens K6BP. Licensed under the GNU Affero General Public License v3.0 or later (AGPL-3.0-or-later).
from odoo import models

SVC = "user_websites.user_websites_service_account"
SITEMAP_POSTS_MAX = 5000


class Website(models.Model):
    _inherit = "website"

    # [@ANCHOR: user_websites:website_enumerate_user_blogs]
    # Verified by [@ANCHOR: test_user_blog_urls_use_the_callsign]
    def _enumerate_pages(self, query_string=None, force=False):
        """The sitemap and the link picker list a member's blog at /<callsign>/blog, once.

        Odoo's own enumeration walks the stock blog routes and would list /blog/<name>-<id> and
        /blog/<name>-<id>/<post>, which now answer a redirect. Those entries are dropped for blogs
        that belong to a member or group, and the published posts are listed at their real address
        (blog.post.website_url) together with the blog's index.
        """
        env_svc = self.env["zero_sudo.security.utils"]._get_service_env(SVC)
        owned_blogs = env_svc["blog.blog"].search(
            ["|", ("owner_user_id", "!=", False), ("user_websites_group_id", "!=", False)],
            limit=SITEMAP_POSTS_MAX,
        )
        stock_prefixes = tuple(
            f"/blog/{self.env['ir.http']._slug(blog)}" for blog in owned_blogs
        )

        def is_stock_address(loc):
            return any(loc == prefix or loc.startswith(prefix + "/") for prefix in stock_prefixes)

        for page in super()._enumerate_pages(query_string=query_string, force=force):
            if not is_stock_address(page["loc"]):
                yield page
        if query_string or not owned_blogs:
            return
        posts = env_svc["blog.post"].search(
            [("blog_id", "in", owned_blogs.ids), ("is_published", "=", True)],
            limit=SITEMAP_POSTS_MAX,
        )
        listed = set()
        for post in posts:
            owner = post.owner_user_id or post.user_websites_group_id
            if post.owner_user_id and not post.owner_user_id._website_publicly_visible():
                continue
            if post.user_websites_group_id and post.user_websites_group_id.is_suspended_from_websites:
                continue
            index_url = f"/{owner.website_slug}/blog"
            if index_url not in listed:
                listed.add(index_url)
                yield {"loc": index_url, "name": index_url}
            yield {"loc": post.website_url, "name": post.name}
