# SPDX-License-Identifier: AGPL-3.0-or-later

# -*- coding: utf-8 -*-
# Copyright © Bruce Perens K6BP. Licensed under the GNU Affero General Public License v3.0 or later (AGPL-3.0-or-later).
"""A member's or group's blog is at /<callsign>/blog and its posts at /<callsign>/blog/<post-slug>.

Bruce, 2026-10-29: "the blog slug must be /<callsign>/blog". Odoo's own blog addresses are
/blog/<name-slug>-<id> and /blog/<blog>/<post>, and the blog's name (here "Bruce Perens (K6BP)'s
Blog") was in them. This module gives member and group blogs one address built from the site's
slug, and answers a permanent redirect at every stock address of such a blog, so old links, feed
readers and search engines follow. A blog that belongs to nobody (the platform's own) is untouched.
"""
from urllib.parse import quote

from odoo import http
from odoo.http import Response, request
from odoo.tools import html2plaintext

from odoo.addons.website_blog.controllers.main import WebsiteBlog

from .main import _legacy_slug_redirect

SVC = "user_websites.user_websites_service_account"
FEED_POSTS_MAX = 50


# [@ANCHOR: user_websites:COMM_owned_blog_path]
# Verified by [@ANCHOR: test_user_blog_urls_use_the_callsign]
def _owned_blog_path(blog):
    """/<callsign>/blog for a blog owned by a member or group, else ''."""
    if not blog:
        return ""
    return blog.site_path or ""


class SiteBlogUrl:
    """Stand-in for the stock page's `blog_url` (a QueryURL): the blog links it makes.

    The stock blog post page asks `blog_url(blog=..., tag=..., date_begin=...)` for its breadcrumb,
    tag badges, archive and blog list, and answers /blog/<name>-<id>[/tag/...]. A member's blog has
    one address and no tag or archive pages, so every such link is the blog's own address (a blog
    that is not a member's keeps the stock one).
    """

    # [@ANCHOR: user_websites:COMM_site_blog_url_init]
    # Verified by [@ANCHOR: test_user_blog_urls_use_the_callsign]
    def __init__(self, default_path):
        self.default_path = default_path

    # [@ANCHOR: user_websites:COMM_site_blog_url_call]
    # Verified by [@ANCHOR: test_user_blog_urls_use_the_callsign]
    def __call__(self, path=None, path_args=None, **kwargs):
        blog = kwargs.get("blog")
        if blog:
            return blog.site_path or f"/blog/{request.env['ir.http']._slug(blog)}"
        return self.default_path


class UserBlogPages(http.Controller):

    # [@ANCHOR: user_websites:COMM_user_blog_owner]
    # Verified by [@ANCHOR: test_user_blog_urls_use_the_callsign]
    def _blog_owner(self, website_slug, rest):
        """(profile_user, profile_group, redirect): the site that owns `website_slug`'s blog.

        A retired slug answers its 301 in `redirect`; an unknown, hidden or suspended site is a 404
        (the same rule user_blog_index() applies).
        """
        env_svc = request.env["zero_sudo.security.utils"]._get_service_env(SVC)
        user_id = env_svc["res.users"].get_record_by_slug(website_slug)
        profile_user = env_svc["res.users"].browse(user_id) if user_id else env_svc["res.users"]
        group_id = env_svc["user.websites.group"].get_record_by_slug(website_slug)
        profile_group = (
            env_svc["user.websites.group"].browse(group_id)
            if group_id
            else env_svc["user.websites.group"]
        )
        if not profile_user and not profile_group:
            legacy = _legacy_slug_redirect(website_slug, rest)
            if legacy:
                return profile_user, profile_group, legacy
            raise request.not_found()
        if profile_user and not profile_user._website_publicly_visible():
            raise request.not_found()
        if profile_group and profile_group.is_suspended_from_websites:
            raise request.not_found()
        return profile_user, profile_group, None

    @http.route(
        "/<uwslug:website_slug>/blog/<string:post_slug>",
        type="http",
        auth="public",
        website=True,
        sitemap=False,
    )
    # [@ANCHOR: user_websites:COMM_user_blog_post]
    # Verified by [@ANCHOR: test_user_blog_urls_use_the_callsign]
    # Verified by [@ANCHOR: COMM_test_user_blog_urls_use_the_callsign]
    def user_blog_post(self, website_slug, post_slug, **kwargs):
        """One post of a member's or group's blog, at /<callsign>/blog/<post-slug>.

        Rendered by Odoo's own blog post page, so the layout, comments and editor are the stock
        ones. The post is found as the visitor (an unpublished post is the owner's alone), must
        belong to this site, and a request for any other spelling of the slug is redirected to the
        one address (/K6BP/blog/... or a changed title).
        """
        profile_user, profile_group, redirect = self._blog_owner(
            website_slug, f"/blog/{quote(post_slug, safe='')}"
        )
        if redirect:
            return redirect
        post_id = request.env["ir.http"]._unslug(post_slug)[1]
        if not post_id:
            raise request.not_found()
        post = request.env["blog.post"].search([("id", "=", post_id)], limit=1)
        if not post:
            raise request.not_found()
        mine = (
            post.owner_user_id.id == profile_user.id
            if profile_user
            else post.user_websites_group_id.id == profile_group.id
        )
        if not mine:
            raise request.not_found()
        canonical_slug, _blog, canonical_post = post.website_url.strip("/").split("/")
        if website_slug != canonical_slug or post_slug != canonical_post:
            return request.redirect(post.website_url, code=301, local=True)
        response = WebsiteBlog().blog_post(post.blog_id, post, **kwargs)
        if isinstance(response, Response) and response.qcontext is not None:
            qcontext = response.qcontext
            # Rendered when the response is sent, so the page's own links can still be changed.
            # The blog bar and blog list name this blog only: they would otherwise list every
            # blog of the website, other members' included, each as a stock address.
            qcontext["blog_url"] = SiteBlogUrl(post.blog_id.site_path)
            qcontext["blogs"] = post.blog_id
        return response

    @http.route(
        "/<uwslug:website_slug>/blog/feed",
        type="http",
        auth="public",
        website=True,
        sitemap=False,
    )
    # [@ANCHOR: user_websites:COMM_user_blog_feed]
    # Verified by [@ANCHOR: test_user_blog_urls_use_the_callsign]
    def user_blog_feed(self, website_slug, limit="15", **kwargs):
        """The Atom feed of a member's or group's published posts, at /<callsign>/blog/feed."""
        profile_user, profile_group, redirect = self._blog_owner(website_slug, "/blog/feed")
        if redirect:
            return redirect
        env_svc = request.env["zero_sudo.security.utils"]._get_service_env(SVC)
        main = profile_user or profile_group
        domain = (
            [("owner_user_id", "=", profile_user.id)]
            if profile_user
            else [("user_websites_group_id", "=", profile_group.id)]
        )
        blogs = env_svc["blog.blog"].search(domain, limit=100)
        if not blogs:
            raise request.not_found()
        try:
            count = max(1, min(int(limit), FEED_POSTS_MAX))
        except ValueError:
            count = 15
        # As the visitor: only published posts come back, whatever the service account could see.
        posts = request.env["blog.post"].search(
            [("blog_id", "in", blogs.ids)], limit=count, order="post_date DESC"
        )
        return request.render(
            "user_websites.user_blog_feed",
            {
                "site_name": main.name,
                "site_blog_url": f"/{main.website_slug}/blog",
                "base_url": request.website.get_base_url(),
                "posts": posts,
                "html2plaintext": html2plaintext,
            },
            headers=[("Content-Type", "application/atom+xml")],
        )


class UserBlogRedirects(WebsiteBlog):
    """Odoo's own blog addresses, redirected for the blogs that have a /<callsign>/blog address.

    A bare @http.route() keeps the stock route list and adds this check in front of it.
    """

    # [@ANCHOR: user_websites:COMM_stock_blog_redirects]
    # Verified by [@ANCHOR: test_user_blog_urls_use_the_callsign]
    @http.route()
    def blog(self, blog=None, tag=None, page=1, search=None, **opt):
        path = _owned_blog_path(blog)
        if path:
            page_number = int(page or 1)
            target = path if page_number == 1 else f"{path}/page/{page_number}"
            return request.redirect(target, code=301, local=True)
        return super().blog(blog=blog, tag=tag, page=page, search=search, **opt)

    # [@ANCHOR: user_websites:COMM_stock_blog_feed_redirect]
    # Verified by [@ANCHOR: test_user_blog_urls_use_the_callsign]
    @http.route()
    def blog_feed(self, blog, limit="15", **kwargs):
        path = _owned_blog_path(blog)
        if path:
            return request.redirect(f"{path}/feed", code=301, local=True)
        return super().blog_feed(blog, limit=limit, **kwargs)

    # [@ANCHOR: user_websites:COMM_stock_old_blog_post_redirect]
    # Verified by [@ANCHOR: test_user_blog_urls_use_the_callsign]
    @http.route()
    def old_blog_post(self, blog, blog_post, **post):
        if _owned_blog_path(blog):
            return request.redirect(blog_post.website_url, code=301, local=True)
        return super().old_blog_post(blog, blog_post, **post)

    # [@ANCHOR: user_websites:COMM_stock_blog_post_redirect]
    # Verified by [@ANCHOR: test_user_blog_urls_use_the_callsign]
    @http.route()
    def blog_post(self, blog, blog_post, tag_id=None, page=1, enable_editor=None, **post):
        if _owned_blog_path(blog) or _owned_blog_path(blog_post.blog_id):
            return request.redirect(blog_post.website_url, code=301, local=True)
        return super().blog_post(
            blog, blog_post, tag_id=tag_id, page=page, enable_editor=enable_editor, **post
        )
