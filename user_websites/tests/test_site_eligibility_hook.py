# Copyright © Bruce Perens K6BP.
# SPDX-License-Identifier: AGPL-3.0-or-later

# -*- coding: utf-8 -*-
"""The eligibility extension point: open to everyone by default, hides without deleting when narrowed.

Bruce, 2026-10-13: hams.com offers a website and blog only to a verified licensed amateur who has
accepted the agreement and done the $1 verification. This module only provides the hook.
"""
import odoo.http
from odoo.tests import tagged

from odoo.addons.user_websites.models.site_eligibility import ResUsersSiteEligibility
from odoo.addons.zero_sudo.tests.common import HamsHttpCase

GAP = [{"code": "need_x", "message": "Verify your licence", "url": "/my/verify"}]
ARCH = (
    '<t name="H" t-name="user_websites.elig_{key}"><t t-call="user_websites.template_default_home">'
    '<div id="wrap" class="oe_structure"><p>{body}</p></div></t></t>'
)


@tagged("post_install", "-at_install")
class TestSiteEligibilityHook(HamsHttpCase):
    # [@ANCHOR: test_site_eligibility_hook]

    def setUp(self):
        super().setUp()
        groups = [self.env.ref("base.group_portal").id, self.env.ref("user_websites.group_user_websites_user").id]
        self.owner = self.env["res.users"].create(
            {
                "name": "Elig Owner",
                "login": "eligowner",
                "password": "eligowner",
                "email": "eligowner@example.com",
                "website_slug": "eligowner",
                "privacy_show_in_directory": True,
                "group_ids": [(6, 0, groups)],
            }
        )
        self.home = self._page("/eligowner", "home body")
        self.about = self._page("/eligowner/about", "about body")
        self.mine_off = self._page("/eligowner/draft", "draft body", published=False)
        blog = self.env["blog.blog"].create({"name": "B", "owner_user_id": self.owner.id})
        self.post = self.env["blog.post"].create(
            {"name": "P", "blog_id": blog.id, "owner_user_id": self.owner.id, "is_published": True}
        )
        self.env.flush_all()

    def _page(self, url, body, published=True):
        return self.env["website.page"].create(
            {
                "url": url,
                "name": body,
                "type": "qweb",
                "owner_user_id": self.owner.id,
                "website_published": published,
                "arch": ARCH.format(key=url.strip("/").replace("/", "_"), body=body),
            }
        )

    def _gaps(self, gaps):
        return self.safe_patch_object_cm(ResUsersSiteEligibility, "_website_eligibility_gaps", lambda s: list(gaps))

    def test_01_open_source_default_everyone_is_eligible(self):
        # Tests [@ANCHOR: user_websites:site_eligibility]
        # Tests [@ANCHOR: user_websites:COMM_website_eligibility_gaps]
        # Tests [@ANCHOR: user_websites:COMM_hams_website_eligible]
        self.assertEqual(self.owner._website_eligibility_gaps(), [])
        self.assertTrue(self.owner._hams_website_eligible())
        self.assertEqual(self.env["res.users"].reconcile_website_eligibility([self.owner.id]), {})
        self.assertEqual(self.url_open("/eligowner", allow_redirects=False).status_code, 200)

    def test_02_ineligible_public_site_is_404_and_content_is_kept_then_returns(self):
        # Tests [@ANCHOR: user_websites:COMM_reconcile_website_eligibility]
        # Tests [@ANCHOR: user_websites:COMM_website_publicly_visible]
        # Tests [@ANCHOR: user_websites:fn_ir_http_serve_page_eligibility]
        # Tests [@ANCHOR: user_websites:field_hidden_by_eligibility]
        with self._gaps(GAP):
            self.assertEqual(self.url_open("/eligowner", allow_redirects=False).status_code, 404)
            self.assertEqual(self.url_open("/eligowner/about", allow_redirects=False).status_code, 404)
            self.assertEqual(self.url_open("/eligowner/blog", allow_redirects=False).status_code, 404)
            self.assertEqual(
                self.env["res.users"].reconcile_website_eligibility([self.owner.id]), {self.owner.id: "hidden"}
            )
            for rec in (self.home, self.about, self.mine_off, self.post, self.owner):
                rec.invalidate_recordset()
            self.assertFalse(self.home.website_published)
            self.assertTrue(self.home.hidden_by_eligibility)
            self.assertFalse(self.post.is_published)
            self.assertTrue(self.owner.website_ineligible)
            self.assertFalse(self.mine_off.hidden_by_eligibility, "the owner's own unpublished page is not marked")
            directory = self.env["user_websites.public_directory_view"].search([("user_id", "=", self.owner.id)])
            self.assertFalse(directory, "not in the community directory")
        self.assertEqual(self.env["res.users"].reconcile_website_eligibility([self.owner.id]), {self.owner.id: "shown"})
        for rec in (self.home, self.about, self.mine_off, self.post, self.owner):
            rec.invalidate_recordset()
        self.assertTrue(self.home.website_published)
        self.assertTrue(self.about.website_published)
        self.assertTrue(self.post.is_published)
        self.assertFalse(self.mine_off.website_published, "never republished: the owner unpublished it")
        self.assertFalse(self.owner.website_ineligible)
        self.assertEqual(self.url_open("/eligowner", allow_redirects=False).status_code, 200)
        self.assertTrue(self.env["user_websites.public_directory_view"].search([("user_id", "=", self.owner.id)]))

    def test_03_owner_is_told_what_is_missing_and_cannot_create(self):
        # Tests [@ANCHOR: user_websites:owner_site_ineligible]
        # Tests [@ANCHOR: user_websites:fn_answers_site_response]
        self.authenticate("eligowner", "eligowner")
        with self._gaps(GAP):
            for path in ("/my/site/eligowner", "/my/site/eligowner/page/new"):
                got = self.url_open(path, allow_redirects=False)
                self.assertEqual(got.status_code, 403, path)
                self.assertIn("Verify your licence", got.text)
                self.assertIn("/my/verify", got.text)
            posted = self.url_open(
                "/my/site/eligowner/page/save",
                data={"title": "T", "page_slug": "t", "content": "<p>x</p>", "csrf_token": odoo.http.Request.csrf_token(self)},
                method="POST",
                allow_redirects=False,
            )
            self.assertEqual(posted.status_code, 403)
            self.assertFalse(self.env["website.page"].search([("url", "=", "/eligowner/t")]))
            made = self.url_open(
                "/eligowner/create_blog_post",
                data={"csrf_token": odoo.http.Request.csrf_token(self)},
                method="POST",
                allow_redirects=False,
            )
            self.assertEqual(made.status_code, 303)
            self.assertTrue(made.headers["Location"].endswith("/my/site/eligowner"))
        self.assertEqual(self.url_open("/my/site/eligowner", allow_redirects=False).status_code, 200)

    def test_04_internal_user_and_admin_have_no_exemption_on_the_public_site(self):
        admin_site = self.env["res.users"].create(
            {
                "name": "Admin Site",
                "login": "adminsite",
                "password": "adminsite",
                "email": "adminsite@example.com",
                "website_slug": "adminsite",
                "group_ids": [(6, 0, [self.env.ref("base.group_system").id, self.env.ref("user_websites.group_user_websites_user").id])],
            }
        )
        _admin_page = self.env["website.page"].create(
            {"url": "/adminsite", "name": "A", "type": "qweb", "owner_user_id": admin_site.id, "website_published": True,
             "arch": ARCH.format(key="adminsite", body="admin body")}
        )
        self.env.flush_all()
        with self._gaps(GAP):
            self.authenticate("adminsite", "adminsite")
            self.assertEqual(self.url_open("/adminsite", allow_redirects=False).status_code, 404)

    def test_05_the_flag_is_not_self_service(self):
        self.assertNotIn("website_ineligible", self.owner.SELF_WRITEABLE_FIELDS)
        self.assertTrue(self.env["res.users"]._fields["website_ineligible"].readonly)

    def test_06_the_scheduled_job_runs_the_reconciler_as_the_service_account(self):
        # Tests [@ANCHOR: user_websites:ir_cron_reconcile_website_eligibility]
        # Tests [@ANCHOR: user_websites:COMM_cron_reconcile_website_eligibility]
        cron = self.env.ref("user_websites.ir_cron_reconcile_website_eligibility")
        self.assertTrue(cron.active)
        self.assertEqual(cron.model_id.model, "res.users")
        self.assertEqual(cron.code.strip(), "model._cron_reconcile_website_eligibility()")
        self.assertEqual(cron.user_id, self.env.ref("user_websites.user_websites_service_account"))
        with self._gaps(GAP):
            self.assertEqual(self.env["res.users"]._cron_reconcile_website_eligibility().get(self.owner.id), "hidden")
            self.home.invalidate_recordset()
            self.assertFalse(self.home.website_published)
        self.assertEqual(self.env["res.users"]._cron_reconcile_website_eligibility().get(self.owner.id), "shown")
        self.home.invalidate_recordset()
        self.assertTrue(self.home.website_published)

