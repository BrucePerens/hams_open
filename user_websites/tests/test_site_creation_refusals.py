# Copyright © Bruce Perens K6BP.
# SPDX-License-Identifier: AGPL-3.0-or-later

# -*- coding: utf-8 -*-
"""The refusals of the create-site and create-blog routes that no other test reached: another member's address,
a suspended owner, an owner who is not eligible, and a group the caller is not in or that is suspended.
Each refusal is checked on both routes and must leave no page and no blog behind."""
import odoo.http
from odoo.tests import tagged

from odoo.addons.user_websites.models.site_eligibility import ResUsersSiteEligibility
from odoo.addons.zero_sudo.tests.common import HamsHttpCase

ROUTES = ("create_site", "create_blog")


@tagged("post_install", "-at_install")
class TestSiteCreationRefusals(HamsHttpCase):
    def setUp(self):
        super().setUp()
        groups = [
            self.env.ref("base.group_portal").id,
            self.env.ref("user_websites.group_user_websites_user").id,
        ]
        self.owner, self.visitor, self.member = self.env["res.users"].create(
            [
                {
                    "name": login.title(),
                    "login": login,
                    "password": login,
                    "email": f"{login}@example.com",
                    "website_slug": login,
                    "group_ids": [(6, 0, groups)],
                }
                for login in ("screfowner", "screfvisitor", "screfmember")
            ]
        )
        self.group = self.env["user.websites.group"].create(
            {"name": "Scref Group", "website_slug": "screfgroup", "member_ids": [(6, 0, [self.member.id])]}
        )

    def _post(self, slug, route, login):
        self.env.flush_all()
        self.authenticate(login, login)
        return self.url_open(
            f"/{slug}/{route}",
            data={"csrf_token": odoo.http.Request.csrf_token(self)},
            method="POST",
            allow_redirects=False,
        )

    def _created(self):
        self.env.invalidate_all()
        pages = self.env["website.page"].with_context(active_test=False).search_count(
            ["|", ("owner_user_id", "in", (self.owner.id, self.visitor.id)), ("user_websites_group_id", "=", self.group.id)]
        )
        blogs = self.env["blog.blog"].with_context(active_test=False).search_count(
            ["|", ("owner_user_id", "in", (self.owner.id, self.visitor.id)), ("user_websites_group_id", "=", self.group.id)]
        )
        return pages, blogs

    def assertRefused(self, slug, login, status):
        before = self._created()
        for route in ROUTES:
            response = self._post(slug, route, login)
            self.assertEqual(response.status_code, status, f"{route} as {login} on {slug}")
        self.assertEqual(self._created(), before)

    def test_another_members_address_is_not_found(self):
        # Tests [@ANCHOR: user_websites:COMM_create_site]
        # Tests [@ANCHOR: user_websites:COMM_create_blog]
        self.assertRefused("screfowner", "screfvisitor", 404)

    def test_a_suspended_owner_cannot_create(self):
        # Tests [@ANCHOR: user_websites:COMM_create_site]
        # Tests [@ANCHOR: user_websites:COMM_create_blog]
        self.owner.write({"is_suspended_from_websites": True})
        self.assertRefused("screfowner", "screfowner", 404)

    def test_an_ineligible_owner_is_sent_to_the_tools_page(self):
        # Tests [@ANCHOR: user_websites:COMM_create_site]
        # Tests [@ANCHOR: user_websites:COMM_create_blog]
        with self.safe_patch_object_cm(ResUsersSiteEligibility, "_website_eligibility_gaps", lambda s: ["a gap"]):
            self.assertRefused("screfowner", "screfowner", 303)

    def test_a_non_member_cannot_create_in_a_group(self):
        # Tests [@ANCHOR: user_websites:COMM_create_site]
        # Tests [@ANCHOR: user_websites:COMM_create_blog]
        self.assertRefused("screfgroup", "screfvisitor", 404)

    def test_a_member_cannot_create_in_a_suspended_group(self):
        # Tests [@ANCHOR: user_websites:COMM_create_site]
        # Tests [@ANCHOR: user_websites:COMM_create_blog]
        self.group.write({"is_suspended_from_websites": True})
        self.assertRefused("screfgroup", "screfmember", 404)

    def test_a_member_can_create_in_the_group(self):
        # Tests [@ANCHOR: user_websites:COMM_create_blog]
        response = self._post("screfgroup", "create_blog", "screfmember")
        self.assertEqual(response.status_code, 303)
        self.assertEqual(self._created()[1], 1)
