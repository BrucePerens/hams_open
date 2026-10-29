# Copyright © Bruce Perens K6BP.
# SPDX-License-Identifier: AGPL-3.0-or-later

# -*- coding: utf-8 -*-
"""A member's blog is at /<callsign>/blog and its posts at /<callsign>/blog/<post-slug>.

Bruce, 2026-10-29: "the blog slug must be /<callsign>/blog". His blog was named "Bruce Perens
(K6BP)'s Blog" and its address was Odoo's /blog/<that name, slugged>-<id>.
"""
import re

import odoo.http
from odoo.tests import tagged

from odoo.addons.zero_sudo.tests.common import HamsHttpCase


@tagged("post_install", "-at_install")
class TestUserBlogUrls(HamsHttpCase):
    # [@ANCHOR: test_user_blog_urls_use_the_callsign]

    def setUp(self):
        super().setUp()
        groups = [
            self.env.ref("base.group_portal").id,
            self.env.ref("user_websites.group_user_websites_user").id,
        ]
        self.owner, self.other = self.env["res.users"].create(
            [
                {
                    "name": login.title(),
                    "login": login,
                    "password": login,
                    "email": f"{login}@example.com",
                    "website_slug": login,
                    "group_ids": [(6, 0, groups)],
                }
                for login in ("burlowner", "burlother")
            ]
        )
        self.blog = self.env["blog.blog"].create(
            {"name": "Burlowner (K6BP)'s Blog", "owner_user_id": self.owner.id}
        )
        self.post = self.env["blog.post"].create(
            {
                "name": "Dipole Notes",
                "blog_id": self.blog.id,
                "owner_user_id": self.owner.id,
                "is_published": True,
                "content": "<p>Body text</p>",
            }
        )
        self.draft = self.env["blog.post"].create(
            {
                "name": "Unfinished Draft",
                "blog_id": self.blog.id,
                "owner_user_id": self.owner.id,
                "is_published": False,
                "content": "<p>Draft text</p>",
            }
        )
        self.env.flush_all()
        self.post_slug = self.env["ir.http"]._slug(self.post)
        self.blog_slug = self.env["ir.http"]._slug(self.blog)

    def _location(self, response):
        return response.headers["Location"].replace(self.base_url(), "")

    def test_01_addresses_are_built_from_the_callsign(self):
        # Tests [@ANCHOR: user_websites:blog_post_website_url]
        # Tests [@ANCHOR: user_websites:blog_blog_site_path]
        # Tests [@ANCHOR: user_websites:fn_mixin_user_site_slugs]
        self.assertEqual(self.blog.site_path, "/burlowner/blog")
        self.assertEqual(self.post.website_url, f"/burlowner/blog/{self.post_slug}")
        self.assertRegex(self.post_slug, r"^dipole-notes-\d+$")
        self.assertNotIn("'", self.post.website_url)
        self.assertNotIn("-s-blog", self.post.website_url)

    def test_02_post_is_served_at_the_callsign_address(self):
        # Tests [@ANCHOR: user_websites:COMM_user_blog_post]
        response = self.url_open(self.post.website_url, allow_redirects=False)
        self.assertEqual(response.status_code, 200)
        self.assertIn("Dipole Notes", response.text)
        self.assertIn("Body text", response.text)

    def test_03_other_spellings_redirect_to_the_one_address(self):
        # Tests [@ANCHOR: user_websites:COMM_user_blog_post]
        old_title = self.url_open(f"/burlowner/blog/old-title-{self.post.id}", allow_redirects=False)
        self.assertEqual(old_title.status_code, 301)
        self.assertEqual(self._location(old_title), self.post.website_url)
        upper = self.url_open(f"/BURLOWNER/blog/{self.post_slug}", allow_redirects=False)
        self.assertEqual(upper.status_code, 301)
        self.assertEqual(self._location(upper), self.post.website_url)

    def test_04_a_post_is_only_found_under_its_own_site(self):
        # Tests [@ANCHOR: user_websites:COMM_user_blog_post]
        self.assertEqual(self.url_open(f"/burlother/blog/{self.post_slug}").status_code, 404)
        self.assertEqual(self.url_open("/burlowner/blog/no-such-post").status_code, 404)
        self.assertEqual(self.url_open("/burlowner/blog/999999999").status_code, 404)

    def test_05_a_draft_is_the_owners_alone(self):
        # Tests [@ANCHOR: user_websites:COMM_user_blog_post]
        self.assertEqual(self.url_open(self.draft.website_url).status_code, 404)
        self.authenticate("burlother", "burlother")
        self.assertEqual(self.url_open(self.draft.website_url).status_code, 404)
        self.authenticate("burlowner", "burlowner")
        self.assertEqual(self.url_open(self.draft.website_url).status_code, 200)

    def test_06_stock_addresses_of_a_member_blog_are_permanent_redirects(self):
        # Tests [@ANCHOR: user_websites:COMM_stock_blog_redirects]
        expected = {
            f"/blog/{self.blog_slug}": "/burlowner/blog",
            f"/blog/{self.blog_slug}/page/2": "/burlowner/blog/page/2",
            f"/blog/{self.blog_slug}/feed": "/burlowner/blog/feed",
            f"/blog/{self.blog_slug}/{self.post_slug}": self.post.website_url,
        }
        for old, new in expected.items():
            response = self.url_open(old, allow_redirects=False)
            self.assertEqual(response.status_code, 301, old)
            self.assertEqual(self._location(response), new, old)

    def test_06b_the_old_post_edit_style_address_ends_at_the_callsign_address(self):
        # Tests [@ANCHOR: user_websites:COMM_stock_blog_redirects]
        # Odoo first corrects the slugs in /blog/<id>/post/<id> (a 301 of its own), then ours follows.
        response = self.url_open(f"/blog/{self.blog.id}/post/{self.post.id}")
        self.assertEqual(response.status_code, 200)
        self.assertEqual(response.url.replace(self.base_url(), ""), self.post.website_url)
        self.assertTrue(response.history)
        self.assertEqual({hop.status_code for hop in response.history}, {301})

    def test_07_the_blog_index_and_owner_links_use_the_callsign(self):
        # Tests [@ANCHOR: user_websites:COMM_user_blog_index]
        html = self.url_open("/burlowner/blog").text
        self.assertIn(f'href="{self.post.website_url}"', html)
        self.assertNotIn("/blog/%d/post/" % self.blog.id, html)
        self.authenticate("burlowner", "burlowner")
        dashboard = self.url_open("/my/site/burlowner").text
        self.assertIn(f'href="{self.post.website_url}"', dashboard)

    def test_08_atom_feed_lists_published_posts_at_their_address(self):
        # Tests [@ANCHOR: user_websites:COMM_user_blog_feed]
        response = self.url_open("/burlowner/blog/feed")
        self.assertEqual(response.status_code, 200)
        self.assertIn("application/atom+xml", response.headers["Content-Type"])
        self.assertIn(f"/burlowner/blog/{self.post_slug}", response.text)
        self.assertNotIn("Unfinished Draft", response.text)
        self.assertEqual(self.url_open("/burlother/blog/feed").status_code, 404)

    def test_09_post_page_links_stay_on_the_callsign_address(self):
        # [@ANCHOR: COMM_test_user_blog_urls_use_the_callsign]
        # Tests [@ANCHOR: user_websites:COMM_user_blog_post]
        second = self.env["blog.post"].create(
            {
                "name": "Second Post",
                "blog_id": self.blog.id,
                "owner_user_id": self.owner.id,
                "is_published": True,
                "content": "<p>More</p>",
            }
        )
        self.env.flush_all()
        html = self.url_open(self.post.website_url).text
        self.assertIn(second.website_url, html, "the read-next link")
        stock_links = re.findall(r'(?:href|data-url)="(/blog/[a-z0-9-]*-[0-9]+[^"]*)"', html)
        self.assertEqual([], stock_links, "no link may point at a stock blog address")

    def test_10_sitemap_lists_the_callsign_addresses_only(self):
        # Tests [@ANCHOR: user_websites:website_enumerate_user_blogs]
        website = self.env["website"].search([], limit=1)
        locs = [page["loc"] for page in website._enumerate_pages()]
        self.assertIn(self.post.website_url, locs)
        self.assertIn("/burlowner/blog", locs)
        self.assertNotIn(self.draft.website_url, locs)
        self.assertFalse([loc for loc in locs if f"/blog/{self.blog_slug}" in loc])

    def test_11_a_new_blog_is_named_without_a_possessive(self):
        # Tests [@ANCHOR: user_websites:fn_main_new_blog_name]
        self.authenticate("burlother", "burlother")
        self.env.flush_all()
        self.url_open(
            "/burlother/create_blog",
            data={"csrf_token": odoo.http.Request.csrf_token(self)},
            method="POST",
            allow_redirects=False,
        )
        blog = self.env["blog.blog"].search([("owner_user_id", "=", self.other.id)])
        self.assertEqual(blog.name, "Burlother Blog")
        self.assertEqual(blog.site_path, "/burlother/blog")

    def _only_blogs(self, *keep):
        others = self.env["blog.blog"].search([("id", "not in", [b.id for b in keep])])
        others.write({"active": False})
        self.env.flush_all()

    def test_12_platform_blog_index_with_one_blog_redirects_to_it(self):
        # Tests [@ANCHOR: user_websites:COMM_stock_blog_redirects]
        # A blog owned by a user with no site slug (perens.com's blog, 2026-10-29 outage).
        admin_blog = self.env["blog.blog"].create({"name": "Admin Blog", "owner_user_id": self.env.ref("base.user_admin").id})
        self._only_blogs(admin_blog)
        response = self.url_open("/blog", allow_redirects=False)
        self.assertEqual(response.status_code, 302)
        self.assertEqual(self._location(response), f"/blog/{self.env['ir.http']._slug(admin_blog)}")

    def test_13_platform_blog_index_with_two_blogs_lists_them(self):
        # Tests [@ANCHOR: user_websites:COMM_stock_blog_redirects]
        first = self.env["blog.blog"].create({"name": "Admin Blog", "owner_user_id": self.env.ref("base.user_admin").id})
        second = self.env["blog.blog"].create({"name": "Admin Blog Two", "owner_user_id": self.env.ref("base.user_admin").id})
        self._only_blogs(first, second)
        self.assertEqual(self.url_open("/blog", allow_redirects=False).status_code, 200)
