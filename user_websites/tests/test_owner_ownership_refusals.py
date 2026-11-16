# Copyright © Bruce Perens K6BP.
# SPDX-License-Identifier: AGPL-3.0-or-later

# -*- coding: utf-8 -*-
"""Ownership refusals on the owner routes that write as the service account: publish, delete and restore of a
post or page that belongs to someone else, or does not exist, and a group site's membership and suspension gates.
The service account bypasses the record rules, so these checks are the only guard on those writes."""
import odoo.http
from odoo.tests import tagged

from odoo.addons.zero_sudo.tests.common import HamsHttpCase

ARCH = (
    '<t name="H" t-name="user_websites.own_{key}"><t t-call="user_websites.template_default_home">'
    '<div id="wrap" class="oe_structure"><p>{body}</p></div></t></t>'
)


@tagged("post_install", "-at_install")
class TestOwnerOwnershipRefusals(HamsHttpCase):
    def setUp(self):
        super().setUp()
        groups = [self.env.ref("base.group_portal").id, self.env.ref("user_websites.group_user_websites_user").id]
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
                for login in ("ownrefa", "ownrefb")
            ]
        )
        website = self.env["website"].search([], limit=1)
        self.owner_page = self.env["website.page"].create(
            {
                "url": "/ownrefa/about", "name": "about", "type": "qweb", "owner_user_id": self.owner.id,
                "website_published": True, "website_id": website.id, "arch": ARCH.format(key="a", body="about"),
            }
        )
        blog = self.env["blog.blog"].create({"name": "BA", "owner_user_id": self.owner.id})
        self.owner_post = self.env["blog.post"].create(
            {"name": "Owner Post", "blog_id": blog.id, "owner_user_id": self.owner.id, "is_published": False}
        )
        self.env.flush_all()

    def _post(self, path):
        data = {"csrf_token": odoo.http.Request.csrf_token(self)}
        return self.url_open(path, data=data, method="POST", allow_redirects=False)

    def test_01_a_member_cannot_publish_unpublish_delete_or_restore_another_members_post_through_their_own_site(self):
        # Tests [@ANCHOR: user_websites:COMM_owner_post_publish]
        # Tests [@ANCHOR: user_websites:fn_owner_own_post]
        self.authenticate("ownrefb", "ownrefb")
        post = self.owner_post
        for action in ("publish", "delete", "restore"):
            kind = "post"
            path = (
                f"/my/site/ownrefb/post/{post.id}/publish" if action == "publish"
                else f"/my/site/ownrefb/{kind}/{post.id}/{action}"
            )
            self.assertEqual(self._post(path).status_code, 404, action)
        self.env.invalidate_all()
        post = self.env["blog.post"].with_context(active_test=False).browse(post.id)
        self.assertTrue(post.active)
        self.assertFalse(post.is_published)
        self.assertFalse(post.deleted_by_owner)

    def test_02_a_member_cannot_delete_or_restore_another_members_page_through_their_own_site(self):
        # Tests [@ANCHOR: user_websites:fn_owner_own_page_any]
        # Tests [@ANCHOR: user_websites:fn_owner_own_page]
        page = self.owner_page
        self.authenticate("ownrefa", "ownrefa")
        self.assertEqual(self._post(f"/my/site/ownrefa/page/{page.id}/delete").status_code, 303)
        self.env.invalidate_all()
        self.assertTrue(self.env["website.page"].with_context(active_test=False).browse(page.id).deleted_by_owner)
        self.authenticate("ownrefb", "ownrefb")
        self.assertEqual(self._post(f"/my/site/ownrefb/page/{page.id}/restore").status_code, 404)
        self.env.invalidate_all()
        page = self.env["website.page"].with_context(active_test=False).browse(page.id)
        self.assertFalse(page.active)
        self.assertTrue(page.deleted_by_owner)

    def test_03_an_id_that_does_not_exist_is_a_404_for_delete_restore_and_publish(self):
        # Tests [@ANCHOR: user_websites:fn_owner_own_post]
        # Tests [@ANCHOR: user_websites:fn_owner_own_page_any]
        self.authenticate("ownrefa", "ownrefa")
        missing = 2147483000
        for path in (
            f"/my/site/ownrefa/post/{missing}/delete", f"/my/site/ownrefa/post/{missing}/restore",
            f"/my/site/ownrefa/page/{missing}/restore", f"/my/site/ownrefa/post/{missing}/publish",
        ):
            self.assertEqual(self._post(path).status_code, 404, path)

    def test_04_a_group_site_is_hidden_from_a_non_member_and_from_everyone_when_suspended(self):
        # Tests [@ANCHOR: user_websites:owner_resolve_site]
        group = self.env["user.websites.group"].create(
            {"name": "Ownref Group", "website_slug": "ownrefgroup", "member_ids": [(6, 0, [self.owner.id])]}
        )
        self.env.flush_all()
        self.authenticate("ownrefb", "ownrefb")
        self.assertEqual(self.url_open("/my/site/ownrefgroup").status_code, 404)
        self.authenticate("ownrefa", "ownrefa")
        self.assertEqual(self.url_open("/my/site/ownrefgroup").status_code, 200)
        group.is_suspended_from_websites = True
        self.env.flush_all()
        self.assertEqual(self.url_open("/my/site/ownrefgroup").status_code, 404)

    def _blog_count(self):
        return self.env["blog.blog"].with_context(active_test=False).search_count([])

    def _refused(self, slug):
        """POSTs every create route for `slug` and checks each is a 404 that created nothing."""
        before = (
            self._blog_count(),
            self.env["website.page"].with_context(active_test=False).search_count([]),
            self.env["blog.post"].with_context(active_test=False).search_count([]),
        )
        for route in ("create_site", "create_blog", "create_blog_post"):
            self.assertEqual(self._post(f"/{slug}/{route}").status_code, 404, f"{slug}/{route}")
        self.env.invalidate_all()
        after = (
            self._blog_count(),
            self.env["website.page"].with_context(active_test=False).search_count([]),
            self.env["blog.post"].with_context(active_test=False).search_count([]),
        )
        self.assertEqual(before, after)

    def _group(self, slug, members):
        group = self.env["user.websites.group"].create(
            {"name": f"Group {slug}", "website_slug": slug, "member_ids": [(6, 0, members.ids)]}
        )
        self.env.flush_all()
        return group

    def test_05_the_create_routes_refuse_another_members_site(self):
        # Tests [@ANCHOR: user_websites:COMM_create_site]
        # Tests [@ANCHOR: user_websites:COMM_create_blog]
        # Tests [@ANCHOR: user_websites:COMM_create_blog_post]
        self.authenticate("ownrefb", "ownrefb")
        self._refused("ownrefa")

    def test_06_the_create_routes_refuse_a_slug_that_belongs_to_nobody(self):
        # Tests [@ANCHOR: user_websites:COMM_create_site]
        # Tests [@ANCHOR: user_websites:COMM_create_blog]
        # Tests [@ANCHOR: user_websites:COMM_create_blog_post]
        self.authenticate("ownrefb", "ownrefb")
        self._refused("ownrefnobody")

    def test_07_the_create_routes_refuse_a_group_site_to_a_non_member(self):
        # Tests [@ANCHOR: user_websites:COMM_create_site]
        # Tests [@ANCHOR: user_websites:COMM_create_blog]
        # Tests [@ANCHOR: user_websites:COMM_create_blog_post]
        self._group("ownrefcreate", self.owner)
        self.authenticate("ownrefb", "ownrefb")
        self._refused("ownrefcreate")

    def test_08_the_create_routes_refuse_a_suspended_group_even_to_a_member(self):
        # Tests [@ANCHOR: user_websites:COMM_create_site]
        # Tests [@ANCHOR: user_websites:COMM_create_blog]
        # Tests [@ANCHOR: user_websites:COMM_create_blog_post]
        group = self._group("ownrefsusp", self.owner)
        group.is_suspended_from_websites = True
        self.env.flush_all()
        self.authenticate("ownrefa", "ownrefa")
        self._refused("ownrefsusp")

    def test_09_the_create_routes_refuse_a_suspended_member_their_own_site(self):
        # Tests [@ANCHOR: user_websites:COMM_create_site]
        # Tests [@ANCHOR: user_websites:COMM_create_blog]
        # Tests [@ANCHOR: user_websites:COMM_create_blog_post]
        self.owner.is_suspended_from_websites = True
        self.env.flush_all()
        self.authenticate("ownrefa", "ownrefa")
        self._refused("ownrefa")
