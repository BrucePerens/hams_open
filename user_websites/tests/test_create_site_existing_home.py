# Copyright © Bruce Perens K6BP.
# SPDX-License-Identifier: AGPL-3.0-or-later

# -*- coding: utf-8 -*-
"""create_site never meets the unique page address: it reuses the owner's existing home.

Bruce, 2026-10-29, on hams.com/k6bp/create_site: "The page URL must be unique per website!". His
home page existed at /k6bp but unpublished, so the public page (which looked only for a published
page) offered "Create your site" while the owner's dashboard (which lists every page) did not, and
the button then tried to create a second page at the same address.
"""
import odoo.http
from odoo.tests import tagged

from odoo.addons.zero_sudo.tests.common import HamsHttpCase

ARCH = (
    '<t name="H" t-name="user_websites.csh_{key}"><t t-call="user_websites.template_default_home">'
    '<div id="wrap" class="oe_structure"><p>{body}</p></div></t></t>'
)


@tagged("post_install", "-at_install")
class TestCreateSiteExistingHome(HamsHttpCase):
    # [@ANCHOR: test_create_site_reuses_existing_home]

    def setUp(self):
        super().setUp()
        groups = [
            self.env.ref("base.group_portal").id,
            self.env.ref("user_websites.group_user_websites_user").id,
        ]
        self.owner, self.visitor = self.env["res.users"].create(
            [
                {
                    "name": login.title(),
                    "login": login,
                    "password": login,
                    "email": f"{login}@example.com",
                    "website_slug": login,
                    "group_ids": [(6, 0, groups)],
                }
                for login in ("cshowner", "cshvisitor")
            ]
        )
        self.website = self.env["website"].search([], limit=1)

    def _page(self, url, **extra):
        vals = {
            "url": url,
            "name": "Cshowner Home",
            "type": "qweb",
            "owner_user_id": self.owner.id,
            "website_published": True,
            "website_id": self.website.id,
            "arch": ARCH.format(key=url.strip("/").replace("/", "_"), body="body"),
        }
        vals.update(extra)
        return self.env["website.page"].create(vals)

    def _create_site(self):
        self.env.flush_all()
        return self.url_open(
            "/cshowner/create_site",
            data={"csrf_token": odoo.http.Request.csrf_token(self)},
            method="POST",
            allow_redirects=False,
        )

    def _pages(self):
        self.env.invalidate_all()
        return self.env["website.page"].with_context(active_test=False).search(
            [("owner_user_id", "=", self.owner.id)]
        )

    def test_01_unpublished_home_offers_publish_and_create_site_publishes_it(self):
        # Tests [@ANCHOR: user_websites:COMM_create_site]
        # Tests [@ANCHOR: user_websites:fn_main_owner_home]
        # Tests [@ANCHOR: user_websites:COMM_user_home_fallback]
        home = self._page("/cshowner", website_published=False)
        self.env.flush_all()
        self.authenticate("cshowner", "cshowner")
        html = self.url_open("/cshowner").text
        self.assertIn("Publish Your Home", html)
        self.assertNotIn("Create Your", html)
        done = self._create_site()
        self.assertIn(done.status_code, (302, 303))
        self.assertTrue(done.headers["Location"].endswith("/cshowner"))
        pages = self._pages()
        self.assertEqual(pages, home, "no second page is created")
        self.assertTrue(pages.website_published)
        self.assertEqual(self.url_open("/cshowner", allow_redirects=False).status_code, 200)

    def test_02_deleted_home_is_restored_and_published(self):
        # Tests [@ANCHOR: user_websites:COMM_create_site]
        # Tests [@ANCHOR: user_websites:fn_main_owner_home]
        home = self._page(
            "/cshowner/deleted-1",
            website_published=False,
            active=False,
            deleted_by_owner="2026-10-29 02:26:43",
            deleted_original_url="/cshowner",
        )
        self.env.flush_all()
        self.authenticate("cshowner", "cshowner")
        self.assertIn("Restore Your Home", self.url_open("/cshowner").text)
        self._create_site()
        pages = self._pages()
        self.assertEqual(pages, home)
        self.assertEqual(home.url, "/cshowner")
        self.assertTrue(home.active)
        self.assertTrue(home.website_published)
        self.assertFalse(home.deleted_by_owner)
        self.assertFalse(home.deleted_original_url)

    def test_03_k6bp_shape_unpublished_home_next_to_a_deleted_old_home(self):
        # Tests [@ANCHOR: user_websites:COMM_create_site]
        # The production state: /k6bp unpublished, and the old home parked at /k6bp/deleted-18.
        home = self._page("/cshowner", website_published=False)
        old = self._page(
            "/cshowner/deleted-18",
            website_published=False,
            active=False,
            deleted_by_owner="2026-10-29 02:26:43",
            deleted_original_url="/cshowner/former-home",
        )
        self.env.flush_all()
        self.authenticate("cshowner", "cshowner")
        self._create_site()
        self.assertEqual(len(self._pages()), 2)
        self.assertTrue(home.website_published)
        self.assertFalse(old.active, "the other deleted page stays deleted")
        self.assertEqual(old.url, "/cshowner/deleted-18")

    def test_04_no_home_still_creates_one_and_a_second_click_reuses_it(self):
        # Tests [@ANCHOR: user_websites:COMM_create_site]
        self.authenticate("cshowner", "cshowner")
        self.assertIn("Create Your", self.url_open("/cshowner").text)
        self._create_site()
        first = self._pages()
        self.assertEqual(len(first), 1)
        self.assertTrue(first.website_published)
        self._create_site()
        self.assertEqual(self._pages(), first)

    def test_05_archived_by_moderation_is_not_resurrected(self):
        # Tests [@ANCHOR: user_websites:COMM_create_site]
        # Tests [@ANCHOR: user_websites:fn_main_owner_home]
        home = self._page("/cshowner", website_published=False, active=False)
        self.env.flush_all()
        self.authenticate("cshowner", "cshowner")
        done = self._create_site()
        self.assertEqual(done.status_code, 303)
        self.assertIn("error=home_blocked", done.headers["Location"])
        self.assertFalse(self._pages().active)
        self.assertEqual(self._pages(), home)

    def test_06_visitor_sees_nothing_of_the_owners_state(self):
        # Tests [@ANCHOR: user_websites:COMM_user_home_fallback]
        self._page("/cshowner", website_published=False)
        self.env.flush_all()
        self.authenticate("cshvisitor", "cshvisitor")
        html = self.url_open("/cshowner").text
        self.assertIn("has not yet created their home", html)
        self.assertNotIn("Publish Your Home", html)
