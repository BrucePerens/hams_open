# Copyright © Bruce Perens K6BP.
# SPDX-License-Identifier: AGPL-3.0-or-later

# -*- coding: utf-8 -*-
"""Delete and Restore for the owner's pages and blog posts.

Bruce, 2026-10-13: "There is no 'delete' button on web pages or blog posts." The platform retains
data, so Delete archives the record and hides it everywhere; it is restorable; the home page stays.
"""
import odoo.http
from odoo.tests import tagged

from odoo.addons.zero_sudo.tests.common import HamsHttpCase

ARCH = (
    '<t name="H" t-name="user_websites.del_{key}"><t t-call="user_websites.template_default_home">'
    '<div id="wrap" class="oe_structure"><p>{body}</p></div></t></t>'
)


@tagged("post_install", "-at_install")
class TestOwnerDelete(HamsHttpCase):
    # [@ANCHOR: test_owner_delete_page_and_post]

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
                for login in ("delowner", "delother")
            ]
        )
        self.home = self._page("/delowner", "home body")
        self.about = self._page("/delowner/about", "about body")
        blog = self.env["blog.blog"].create({"name": "B", "owner_user_id": self.owner.id})
        self.post = self.env["blog.post"].create(
            {"name": "Post One", "blog_id": blog.id, "owner_user_id": self.owner.id, "is_published": True}
        )
        self.env.flush_all()

    def _page(self, url, body):
        return self.env["website.page"].create(
            {
                "url": url,
                "name": body,
                "type": "qweb",
                "owner_user_id": self.owner.id,
                "website_published": True,
                # A site-specific page, as the owner tools make them: on a generic page (no website)
                # Odoo's copy-on-write would archive a COPY of the view and leave the original active.
                "website_id": self.env["website"].search([], limit=1).id,
                "arch": ARCH.format(key=url.strip("/").replace("/", "_"), body=body),
            }
        )

    def _post(self, path, token=True):
        data = {"csrf_token": odoo.http.Request.csrf_token(self)} if token else {}
        return self.url_open(path, data=data, method="POST", allow_redirects=False)

    def _refresh(self, *recs):
        self.env.invalidate_all()
        return recs

    def test_01_confirm_page_wording_and_no_erasure_promise(self):
        # Tests [@ANCHOR: user_websites:COMM_owner_delete_confirm]
        self.authenticate("delowner", "delowner")
        text = self.url_open(f"/my/site/delowner/page/{self.about.id}/delete").text
        self.assertIn("Delete this page?", text)
        self.assertIn("It will be removed from your site and can&#39;t be seen by visitors.", text.replace("'", "&#39;"))
        for promise in ("permanently", "erase", "destroy", "cannot be recovered"):
            self.assertNotIn(promise, text.lower())

    def test_02_delete_page_is_404_everywhere_and_kept_archived(self):
        # Tests [@ANCHOR: user_websites:COMM_owner_delete]
        # Tests [@ANCHOR: user_websites:field_deleted_by_owner]
        self.authenticate("delowner", "delowner")
        self.assertEqual(self.url_open("/delowner/about", allow_redirects=False).status_code, 200)
        done = self._post(f"/my/site/delowner/page/{self.about.id}/delete")
        self.assertEqual(done.status_code, 303)
        self.assertEqual(self.url_open("/delowner/about", allow_redirects=False).status_code, 404)
        self.env.invalidate_all()
        page = self.env["website.page"].with_context(active_test=False).browse(self.about.id)
        self.assertTrue(page.exists(), "retained, never hard deleted")
        self.assertFalse(page.active)
        self.assertTrue(page.deleted_by_owner)
        self.assertEqual(page.deleted_original_url, "/delowner/about")
        self.assertFalse(page.website_published)
        self.assertFalse(self.env["website.page"].search([("id", "=", self.about.id)]), "not in any default list")
        dashboard = self.url_open("/my/site/delowner").text
        self.assertIn("Deleted (1)", dashboard)
        self.assertIn(f"/my/site/delowner/page/{self.about.id}/restore", dashboard)
        # Its address is free again.
        again = self._page("/delowner/about", "second about")
        self.assertTrue(again.id)

    def test_03_delete_post_is_gone_from_the_public(self):
        # Tests [@ANCHOR: user_websites:COMM_owner_delete]
        # Tests [@ANCHOR: user_websites:fn_owner_own_post]
        self.authenticate("delowner", "delowner")
        url = self.post.website_url
        self.assertEqual(self.url_open(url, allow_redirects=False).status_code, 200)
        self._post(f"/my/site/delowner/post/{self.post.id}/delete")
        self.assertNotIn("Post One", self.url_open(url).text)
        self.env.invalidate_all()
        post = self.env["blog.post"].with_context(active_test=False).browse(self.post.id)
        self.assertTrue(post.exists())
        self.assertFalse(post.active)
        self.assertFalse(post.is_published)
        self.assertNotIn("Post One</a>", self.url_open("/delowner/blog").text)

    def test_04_other_member_and_missing_csrf_are_refused(self):
        self.authenticate("delother", "delother")
        self.assertEqual(self._post(f"/my/site/delowner/page/{self.about.id}/delete").status_code, 404)
        self.assertEqual(self._post(f"/my/site/delother/page/{self.about.id}/delete").status_code, 404)
        self.authenticate("delowner", "delowner")
        self.assertIn(self._post(f"/my/site/delowner/page/{self.about.id}/delete", token=False).status_code, (400, 403))
        self.assertTrue(self.env["website.page"].browse(self.about.id).active)

    def test_05_restore_brings_it_back_unpublished(self):
        # Tests [@ANCHOR: user_websites:COMM_owner_restore]
        self.authenticate("delowner", "delowner")
        self._post(f"/my/site/delowner/page/{self.about.id}/delete")
        self._post(f"/my/site/delowner/post/{self.post.id}/delete")
        self.assertEqual(self._post(f"/my/site/delowner/page/{self.about.id}/restore").status_code, 303)
        self._post(f"/my/site/delowner/post/{self.post.id}/restore")
        self.env.invalidate_all()
        page = self.env["website.page"].browse(self.about.id)
        self.assertTrue(page.active)
        self.assertEqual(page.url, "/delowner/about")
        self.assertFalse(page.deleted_by_owner)
        self.assertFalse(page.website_published, "restoring is not publishing")
        post = self.env["blog.post"].browse(self.post.id)
        self.assertTrue(post.active)
        self.assertFalse(post.is_published)
        self.authenticate("delother", "delother")
        self.assertEqual(self._post(f"/my/site/delowner/page/{self.about.id}/restore").status_code, 404)

    def test_06_restore_when_the_address_was_reused_gets_a_new_address(self):
        self.authenticate("delowner", "delowner")
        self._post(f"/my/site/delowner/page/{self.about.id}/delete")
        self._page("/delowner/about", "second about")
        self._post(f"/my/site/delowner/page/{self.about.id}/restore")
        self.env.invalidate_all()
        self.assertEqual(self.env["website.page"].browse(self.about.id).url, f"/delowner/restored-{self.about.id}")

    def test_07_home_page_cannot_be_deleted(self):
        self.authenticate("delowner", "delowner")
        done = self._post(f"/my/site/delowner/page/{self.home.id}/delete")
        self.assertEqual(done.status_code, 303)
        self.assertIn("error=home_delete", done.headers["Location"])
        self.env.invalidate_all()
        self.assertTrue(self.env["website.page"].browse(self.home.id).active)
        self.assertEqual(self.url_open("/delowner", allow_redirects=False).status_code, 200)
        self.assertIn("cannot be deleted", self.url_open(f"/my/site/delowner/page/{self.home.id}/delete").text)
