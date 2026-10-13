# Copyright © Bruce Perens K6BP.
# SPDX-License-Identifier: AGPL-3.0-or-later

# -*- coding: utf-8 -*-
"""The site owner's interface, the standalone member-site layout, and the editor's server path.

Bruce, 2026-10-12: "I am on my own 'K6BP' website through user websites and I don't see any
interface for creating anything", "members must NOT type raw HTML", and "the hams.com/<k6bp>
website still has all of the headers and footer for hams.com. It should not."
"""
import base64
import re

import odoo.http
from odoo.tests import tagged
from odoo.tests.common import BaseCase

from odoo.addons.user_websites.controllers import owner_site
from odoo.addons.user_websites.controllers.main import redis_client
from odoo.addons.user_websites.models.member_html import (
    PAGE_SLUG_RE,
    RESERVED_PAGE_SLUGS,
    sanitize_member_html,
)
from odoo.addons.zero_sudo.tests.common import HamsHttpCase

# Markers of hams.com's own site chrome: its header (id="top") with the main menu, and its footer
# (id="bottom"). Member sites must carry neither.
HAMS_HEADER = 'id="top"'
HAMS_FOOTER = 'id="bottom"'
PNG = b"\x89PNG\r\n\x1a\n" + b"\x00" * 64


def _editor_textareas(html):
    """Textareas on a page other than the report-violation modal's own reason box."""
    return [t for t in re.findall(r"<textarea[^>]*>", html) if 'name="description"' not in t]


def _csrf(case):
    return {"csrf_token": odoo.http.Request.csrf_token(case)}


@tagged("post_install", "-at_install")
class TestMemberHtmlSanitizer(BaseCase):
    # [@ANCHOR: test_member_html_sanitize_strips_hostile_markup]

    def test_script_handlers_and_qweb_are_removed_and_flagged(self):
        # Tests [@ANCHOR: user_websites:member_html_sanitize]
        clean, hostile = sanitize_member_html(
            '<p onclick="x()" t-out="a">Hi<script>alert(1)</script></p>'
            '<iframe src="http://evil/"></iframe><a href="javascript:alert(1)">z</a>'
        )
        self.assertTrue(hostile)
        for needle in ("script", "onclick", "t-out", "iframe", "javascript", "alert"):
            self.assertNotIn(needle, clean)
        self.assertIn("Hi", clean)

    def test_helper_functions_are_exercised(self):
        # The helpers below are exercised through the sanitizer and the owner routes tested in
        # this module (test_owner_page_create_edit_publish and friends).
        # Tests [@ANCHOR: user_websites:fn_main_serve_core_fallback]
        # Tests [@ANCHOR: user_websites:fn_owner_escape]
        # Tests [@ANCHOR: user_websites:fn_owner_escape_attr]
        # Tests [@ANCHOR: user_websites:fn_owner_svc]
        # Tests [@ANCHOR: user_websites:fn_owner_owner_domain]
        # Tests [@ANCHOR: user_websites:fn_owner_qcontext]
        # Tests [@ANCHOR: user_websites:fn_owner_report_if_hostile]
        # Tests [@ANCHOR: user_websites:fn_owner_clean_body]
        # Tests [@ANCHOR: user_websites:fn_owner_own_page]
        # Tests [@ANCHOR: user_websites:fn_owner_back_to_editor]
        # Tests [@ANCHOR: user_websites:fn_owner_attachment_owner]
        # Tests [@ANCHOR: user_websites:fn_hmtl_clean_url]
        # Tests [@ANCHOR: user_websites:fn_hmtl_is_hostile]
        # Tests [@ANCHOR: user_websites:fn_hmtl_sanitize_member_html]
        # Tests [@ANCHOR: user_websites:fn_hmtl_clean_children]
        # Tests [@ANCHOR: user_websites:fn_hmtl_clean_attributes]
        # Tests [@ANCHOR: user_websites:fn_hmtl_drop]
        # Tests [@ANCHOR: user_websites:fn_hmtl_unwrap]
        # Tests [@ANCHOR: user_websites:fn_hmtl_menu_pages]
        # Tests [@ANCHOR: user_websites:fn_users_user_websites_menu_pages]
        # Tests [@ANCHOR: user_websites:fn_group_user_websites_menu_pages]
        clean, _hostile = sanitize_member_html("<p>x</p>")
        self.assertEqual(clean, "<p>x</p>")

    def test_svg_schematic_is_kept_and_hostile_svg_is_stripped(self):
        # Tests [@ANCHOR: user_websites:member_html_svg_allowlist]
        clean, hostile = sanitize_member_html(
            "<p>a</p><svg viewBox='0 0 40 20'><rect id='box' width='30' height='10'/></svg><p>b</p>"
        )
        self.assertFalse(hostile)
        self.assertIn("<svg", clean)
        self.assertIn('id="box"', clean)
        self.assertIn("<p>b</p>", clean)
        clean, hostile = sanitize_member_html(
            "<svg onload='x()'><script>x()</script><rect id='r' width='3' height='3'/></svg>"
        )
        self.assertTrue(hostile)
        for gone in ("<script", "onload"):
            self.assertNotIn(gone, clean)

    def test_toolbar_markup_survives(self):
        # Tests [@ANCHOR: user_websites:member_html_sanitize]
        clean, hostile = sanitize_member_html(
            '<h2>T</h2><p style="text-align:center"><b>b</b> <i>i</i> <u>u</u> <code>c</code></p>'
            '<ul><li>a</li></ul><blockquote>q</blockquote><pre>p</pre>'
            '<a href="https://example.org/x">l</a><img src="/web/image/5" alt="pic"/>'
        )
        self.assertFalse(hostile)
        for needle in ("<h2>", 'class="text-center"', "<b>", "<i>", "<u>", "<code>", "<ul>",
                       "<blockquote>", "<pre>", 'href="https://example.org/x"', 'src="/web/image/5"'):
            self.assertIn(needle, clean)
        self.assertNotIn("style=", clean)

    def test_external_images_and_foreign_classes_are_dropped(self):
        # Tests [@ANCHOR: user_websites:member_html_sanitize]
        clean, _hostile = sanitize_member_html(
            '<img src="http://tracker.example/p.gif"><span class="btn btn-danger" style="color:red">x</span>'
        )
        self.assertNotIn("tracker", clean)
        self.assertNotIn("btn", clean)
        self.assertNotIn("style", clean)

    def test_page_slug_rules(self):
        # Tests [@ANCHOR: user_websites:member_page_reserved_subpaths]
        # [@ANCHOR: test_member_page_slug_reserved]
        for word in ("home", "blog", "manage", "create_site", "subscribe"):
            self.assertIn(word, RESERVED_PAGE_SLUGS)
        self.assertTrue(PAGE_SLUG_RE.match("about-me"))
        for bad in ("", "-x", "x-", "A", "a/b", "a b", "x" * 61, "../x"):
            self.assertIsNone(PAGE_SLUG_RE.match(bad), bad)

    def test_image_magic_numbers(self):
        # Tests [@ANCHOR: user_websites:owner_image_mimetype]
        # [@ANCHOR: test_owner_image_upload_rules]
        self.assertEqual(owner_site._image_mimetype(PNG), "image/png")
        self.assertEqual(owner_site._image_mimetype(b"\xff\xd8\xff\xe0" + b"0" * 10), "image/jpeg")
        self.assertEqual(owner_site._image_mimetype(b"RIFF\x00\x00\x00\x00WEBPVP8 "), "image/webp")
        self.assertIsNone(owner_site._image_mimetype(b"<svg xmlns='http://www.w3.org/2000/svg'/>"))
        self.assertIsNone(owner_site._image_mimetype(b"GIF"))


@tagged("post_install", "-at_install")
class TestOwnerSite(HamsHttpCase):

    def setUp(self):
        super().setUp()
        portal = self.env.ref("base.group_portal").id
        uw_user = self.env.ref("user_websites.group_user_websites_user").id
        users = self.env["res.users"].create(
            [
                {
                    "name": login.title(),
                    "login": login,
                    "email": f"{login}@example.com",
                    "website_slug": login,
                    "group_ids": [(6, 0, groups)],
                }
                for login, groups in (
                    ("ownerone", [portal, uw_user]),
                    ("ownertwo", [portal, uw_user]),
                    # The K6BP case: Bruce is an internal administrator with a member site.
                    ("internalowner", [self.env.ref("base.group_user").id, uw_user]),
                )
            ]
        )
        self.owner, self.other, self.internal = users
        self.page = self.env["website.page"].create(
            {
                "url": "/ownerone",
                "name": "Owner One Home",
                "type": "qweb",
                "owner_user_id": self.owner.id,
                "website_published": True,
                "arch": '<t name="H" t-name="user_websites.home_ownerone">'
                '<t t-call="user_websites.template_default_home">'
                '<div id="wrap" class="oe_structure"><p>Welcome to my site</p></div></t></t>',
            }
        )
        self.sub = self.env["website.page"].create(
            {
                "url": "/ownerone/about",
                "name": "About",
                "type": "qweb",
                "owner_user_id": self.owner.id,
                "website_published": True,
                "arch": '<t name="A" t-name="user_websites.page_ownerone_about">'
                '<t t-call="user_websites.template_default_home">'
                '<div id="wrap" class="oe_structure"><p>About me</p></div></t></t>',
            }
        )
        self.env.flush_all()

    # ----------------------------------------------------------- layout / chrome

    def test_01_member_site_has_no_hams_chrome(self):
        # [@ANCHOR: test_member_site_has_no_hams_chrome]
        # Tests [@ANCHOR: user_websites:member_site_standalone_layout]
        for path in ("/ownerone", "/ownerone/blog", "/ownerone/about"):
            for login in (None, "ownerone"):
                if login:
                    self.authenticate(login, login)
                else:
                    self.authenticate(None, None)
                html = self.url_open(path).text
                self.assertNotIn(HAMS_HEADER, html, f"{path} as {login}: hams.com header")
                self.assertNotIn(HAMS_FOOTER, html, f"{path} as {login}: hams.com footer")
                self.assertIn("user-websites-navbar", html, f"{path} as {login}")
                self.assertIn("uw-member-footer", html, f"{path} as {login}")
                self.assertIn("OWNERONE", html)
                self.assertIn("Skip to content", html)

    def test_02_normal_hams_pages_keep_the_hams_chrome(self):
        self.authenticate(None, None)
        html = self.url_open("/contactus").text
        self.assertIn(HAMS_HEADER, html)
        self.assertNotIn("uw-member-footer", html)

    def test_03_footer_links_and_report(self):
        self.authenticate(None, None)
        html = self.url_open("/ownerone/about").text
        self.assertIn('href="https://hams.com/"', html)
        self.assertIn('href="/privacy"', html)
        self.assertIn('href="/terms"', html)
        self.assertIn("reportViolationModal", html)

    # ------------------------------------------------------------------ toolbar

    def test_04_toolbar_visible_to_owner_only(self):
        # [@ANCHOR: test_owner_toolbar_visible_to_owner_only]
        # Tests [@ANCHOR: user_websites:UX_OWNER_TOOLBAR]
        self.authenticate("ownerone", "ownerone")
        for path in ("/ownerone", "/ownerone/blog", "/ownerone/about"):
            html = self.url_open(path).text
            self.assertIn("uw-owner-toolbar", html, path)
            self.assertIn("New page", html)
            self.assertIn("New blog post", html)
            self.assertIn("Manage site", html)
            self.assertIn("View as visitor", html)
        about = self.url_open("/ownerone/about").text
        self.assertIn(f"/my/site/ownerone/page/{self.sub.id}", about)
        self.assertIn("Unpublish this page", about)
        # View as visitor hides the toolbar.
        self.assertNotIn("uw-owner-toolbar", self.url_open("/ownerone?visitor=1").text)
        # Another member, and a visitor, never see it.
        self.authenticate("ownertwo", "ownertwo")
        self.assertNotIn("uw-owner-toolbar", self.url_open("/ownerone").text)
        self.authenticate(None, None)
        self.assertNotIn("uw-owner-toolbar", self.url_open("/ownerone").text)

    def test_05_toolbar_for_internal_user_on_own_slug_only(self):
        # Tests [@ANCHOR: user_websites:UX_OWNER_TOOLBAR]
        self.authenticate("internalowner", "internalowner")
        self.assertIn("uw-owner-toolbar", self.url_open("/internalowner").text)
        self.assertIn("uw-owner-toolbar", self.url_open("/internalowner/blog").text)
        self.assertNotIn("uw-owner-toolbar", self.url_open("/ownerone").text)
        self.assertEqual(self.url_open("/my/site/internalowner").status_code, 200)
        self.assertEqual(self.url_open("/my/site/ownerone").status_code, 404)

    def test_06_placeholder_owner_gets_create_buttons(self):
        self.authenticate("ownertwo", "ownertwo")
        html = self.url_open("/ownertwo").text
        self.assertIn("uw-owner-toolbar", html)
        self.assertIn("Create Your", html)

    # --------------------------------------------------------------- dashboard

    def test_07_dashboard_access(self):
        # [@ANCHOR: test_owner_site_denied_for_other_member]
        # Tests [@ANCHOR: user_websites:owner_resolve_site]
        self.authenticate("ownertwo", "ownertwo")
        for path in (
            "/my/site/ownerone",
            "/my/site/ownerone/page/new",
            f"/my/site/ownerone/page/{self.page.id}",
        ):
            self.assertEqual(self.url_open(path).status_code, 404, path)
        self.authenticate(None, None)
        response = self.url_open("/my/site/ownerone", allow_redirects=False)
        self.assertIn(response.status_code, (302, 303))
        self.assertIn("/web/login", response.headers["Location"])

    def test_08_dashboard_lists_content(self):
        # [@ANCHOR: test_owner_site_dashboard_lists_content]
        # Tests [@ANCHOR: user_websites:COMM_owner_site_dashboard]
        # Tests [@ANCHOR: user_websites:COMM_owner_site_entry]
        self.env["blog.post"].create(
            {
                "name": "My Dashboard Post",
                "blog_id": self.env["blog.blog"].create(
                    {"name": "Owner Blog", "owner_user_id": self.owner.id}
                ).id,
                "owner_user_id": self.owner.id,
            }
        )
        self.authenticate("ownerone", "ownerone")
        html = self.url_open("/my/site/ownerone").text
        for needle in ("Owner One Home", "/ownerone/about", "My Dashboard Post", "Draft",
                       "Images", "/my/privacy", "Unpublish"):
            self.assertIn(needle, html)
        self.assertNotIn("ownertwo", html)
        self.assertEqual(self.url_open("/my/site").status_code, 200)

    # ------------------------------------------------------------ page editing

    def _post(self, path, **data):
        data.update(_csrf(self))
        return self.url_open(path, data=data, method="POST")

    def test_09_page_create_edit_publish(self):
        # [@ANCHOR: test_owner_page_create_edit_publish]
        # Tests [@ANCHOR: user_websites:COMM_owner_page_save]
        # Tests [@ANCHOR: user_websites:COMM_owner_page_new]
        # Tests [@ANCHOR: user_websites:COMM_owner_page_edit]
        # Tests [@ANCHOR: user_websites:COMM_owner_page_publish]
        # Tests [@ANCHOR: user_websites:owner_page_arch]
        # Tests [@ANCHOR: user_websites:owner_page_arch_body]
        self.authenticate("ownerone", "ownerone")
        editor = self.url_open("/my/site/ownerone/page/new").text
        self.assertIn('contenteditable="true"', editor)
        self.assertIn("data-uw-cmd=\"bold\"", editor)
        response = self._post(
            "/my/site/ownerone/page/save",
            title="Shack Tour",
            page_slug="shack-tour",
            content="<h2>Radios</h2><p>I have <b>many</b>.</p>",
            is_published="on",
        )
        self.assertEqual(response.status_code, 200)
        page = self.env["website.page"].search([("url", "=", "/ownerone/shack-tour")])
        self.assertTrue(page)
        self.assertEqual(page.owner_user_id, self.owner)
        self.assertTrue(page.website_published)
        self.assertEqual(page.website_id, self.env["website"].search([], limit=1) or page.website_id)
        public = self.url_open("/ownerone/shack-tour").text
        self.assertIn("<h2>Radios</h2>", public)
        self.assertIn("uw-member-page", public)
        self.assertNotIn("uw-owner-toolbar", self.url_open("/ownerone/shack-tour?visitor=1").text)
        # The page shows up in the site's own menu.
        self.assertIn("Shack Tour", public)
        # Edit.
        self.assertIn("Radios", self.url_open(f"/my/site/ownerone/page/{page.id}").text)
        self._post(
            "/my/site/ownerone/page/save",
            page_id=str(page.id),
            title="Shack Tour 2",
            content="<p>Edited body</p>",
            is_published="on",
        )
        self.assertIn("Edited body", self.url_open("/ownerone/shack-tour").text)
        # Unpublish keeps the record and hides it from visitors.
        self._post(f"/my/site/ownerone/page/{page.id}/publish", publish="0")
        page.invalidate_recordset()
        self.assertTrue(page.exists())
        self.assertFalse(page.website_published)
        self.authenticate(None, None)
        self.assertEqual(self.url_open("/ownerone/shack-tour").status_code, 404)

    def test_10_page_address_rules(self):
        # Tests [@ANCHOR: user_websites:COMM_owner_page_save]
        self.authenticate("ownerone", "ownerone")
        for bad in ("home", "blog", "Bad Slug", "../x"):
            self._post("/my/site/ownerone/page/save", title="X", page_slug=bad, content="<p>x</p>")
        self.assertFalse(
            self.env["website.page"].search([("url", "=like", "/ownerone/%"), ("name", "=", "X")])
        )
        # Existing address is refused rather than overwritten.
        self._post("/my/site/ownerone/page/save", title="Dup", page_slug="about", content="<p>d</p>")
        self.assertEqual(self.env["website.page"].search_count([("url", "=", "/ownerone/about")]), 1)
        self.sub.invalidate_recordset()
        self.assertEqual(self.sub.name, "About")

    def test_11_cannot_touch_other_members_content(self):
        # Tests [@ANCHOR: user_websites:owner_resolve_site]
        self.authenticate("ownertwo", "ownertwo")
        for path in (f"/my/site/ownerone/page/{self.page.id}/publish", "/my/site/ownerone/page/save"):
            response = self._post(path, publish="0", title="Hijack", content="<p>x</p>")
            self.assertEqual(response.status_code, 404, path)
        self.page.invalidate_recordset()
        self.assertTrue(self.page.website_published)
        # Own site, other member's page id.
        response = self._post(
            f"/my/site/ownertwo/page/{self.page.id}/publish", publish="0"
        )
        self.assertEqual(response.status_code, 404)
        self.page.invalidate_recordset()
        self.assertTrue(self.page.website_published)

    def test_12_post_requires_csrf(self):
        self.authenticate("ownerone", "ownerone")
        response = self.url_open(
            "/my/site/ownerone/page/save",
            data={"title": "No token", "content": "<p>x</p>"},
            method="POST",
        )
        self.assertEqual(response.status_code, 400)
        self.assertFalse(self.env["website.page"].search([("name", "=", "No token")]))
        response = self.url_open(
            f"/my/site/ownerone/page/{self.page.id}/publish", data={"publish": "0"}, method="POST"
        )
        self.assertEqual(response.status_code, 400)
        self.page.invalidate_recordset()
        self.assertTrue(self.page.website_published)

    def test_13_hostile_body_is_sanitized_and_reported(self):
        # Tests [@ANCHOR: user_websites:member_html_sanitize]
        self.authenticate("ownerone", "ownerone")
        before = self.env["content.violation.report"].search_count([])
        self._post(
            "/my/site/ownerone/page/save",
            title="Evil",
            page_slug="evil",
            content='<p onmouseover="x()">hi</p><script>alert(1)</script><p t-out="user.password">z</p>',
            is_published="on",
        )
        page = self.env["website.page"].search([("url", "=", "/ownerone/evil")])
        self.assertTrue(page)
        html = self.url_open("/ownerone/evil").text
        self.assertNotIn("alert(1)", html)
        self.assertNotIn("onmouseover", html)
        self.assertNotIn("user.password", page.arch_base)
        self.assertEqual(
            self.env["content.violation.report"].search_count([]), before + 1,
            "Hostile markup must be filed through the content-violation pipeline.",
        )

    def test_14_internal_owner_body_is_sanitized_too(self):
        # Tests [@ANCHOR: user_websites:member_html_sanitize]
        # An internal administrator's writes skip the model sanitizers; the controller does not.
        self.authenticate("internalowner", "internalowner")
        self._post(
            "/my/site/internalowner/page/save",
            title="Internal",
            page_slug="internal",
            content="<p>ok</p><script>steal()</script>",
            is_published="on",
        )
        page = self.env["website.page"].search([("url", "=", "/internalowner/internal")])
        self.assertTrue(page)
        self.assertNotIn("steal", page.arch_base)

    def test_15_write_rate_limit(self):
        # Tests [@ANCHOR: user_websites:owner_rate_limit]
        # [@ANCHOR: test_owner_write_rate_limited]
        self.authenticate("ownerone", "ownerone")
        key = f"uw:rl:{self.env.cr.dbname}:write:{self.owner.id}"
        redis_client.set(key, 10_000, ex=60)
        try:
            response = self._post(
                "/my/site/ownerone/page/save", title="Spam", page_slug="spam", content="<p>x</p>"
            )
            self.assertEqual(response.status_code, 429)
            self.assertFalse(self.env["website.page"].search([("url", "=", "/ownerone/spam")]))
        finally:
            redis_client.delete(key)

    def test_16_editor_markup_has_no_raw_html_textarea(self):
        # [@ANCHOR: test_owner_page_editor_renders_wysiwyg]
        # Tests [@ANCHOR: user_websites:UX_RICH_EDITOR]
        self.authenticate("ownerone", "ownerone")
        html = self.url_open(f"/my/site/ownerone/page/{self.sub.id}").text
        self.assertEqual(_editor_textareas(html), [], "the editor must not be a raw-markup textarea")
        self.assertIn('role="toolbar"', html)
        self.assertIn('contenteditable="true"', html)
        self.assertIn("About me", html)
        for label in ("Bold", "Italic", "Underline", "Heading", "Bulleted list", "Insert link",
                      "Insert image", "Undo", "Redo", "Block quote", "Code block", "Align left"):
            self.assertIn(f'aria-label="{label}"', html)

    # -------------------------------------------------------------------- posts

    def test_17_new_post_from_toolbar_creates_blog_and_post(self):
        # Tests [@ANCHOR: user_websites:COMM_create_blog_post]
        self.authenticate("ownertwo", "ownertwo")
        response = self._post("/ownertwo/create_blog_post")
        self.assertEqual(response.status_code, 200)
        self.assertIn("/blog_post/edit/", response.url)
        self.assertEqual(_editor_textareas(response.text), [])
        self.assertIn("contenteditable", response.text)
        post = self.env["blog.post"].search([("owner_user_id", "=", self.other.id)])
        self.assertEqual(len(post), 1)
        r = self._post(
            f"/blog_post/edit/submit/{post.id}",
            name="First!",
            content="<p>hello <b>world</b></p><script>x()</script>",
            is_published="on",
        )
        self.assertEqual(r.status_code, 200)
        post.invalidate_recordset()
        self.assertTrue(post.is_published)
        self.assertIn("hello", post.content)
        self.assertNotIn("script", post.content)

    def test_18_post_publish_toggle(self):
        # [@ANCHOR: test_owner_post_publish_toggle]
        # Tests [@ANCHOR: user_websites:COMM_owner_post_publish]
        blog = self.env["blog.blog"].create({"name": "B", "owner_user_id": self.owner.id})
        post = self.env["blog.post"].create(
            {"name": "P", "blog_id": blog.id, "owner_user_id": self.owner.id, "is_published": True}
        )
        self.authenticate("ownertwo", "ownertwo")
        self.assertEqual(
            self._post(f"/my/site/ownerone/post/{post.id}/publish", publish="0").status_code, 404
        )
        self.authenticate("ownerone", "ownerone")
        self._post(f"/my/site/ownerone/post/{post.id}/publish", publish="0")
        post.invalidate_recordset()
        self.assertTrue(post.exists())
        self.assertFalse(post.is_published)

    # ------------------------------------------------------------------- images

    def _upload(self, data, filename="pic.png", **extra):
        payload = {"json_reply": "1", **_csrf(self), **extra}
        return self.url_open(
            "/my/site/ownerone/image",
            data=payload,
            files={"ufile": (filename, data)},
            method="POST",
        )

    def test_19_image_upload_rules(self):
        # Tests [@ANCHOR: user_websites:COMM_owner_image_upload]
        self.authenticate("ownerone", "ownerone")
        ok = self._upload(PNG).json()
        self.assertTrue(ok["ok"], ok)
        attachment = self.env["ir.attachment"].browse(int(ok["url"].rsplit("/", 1)[1]))
        self.assertTrue(attachment.public)
        self.assertEqual(attachment.mimetype, "image/png")
        self.assertEqual(attachment.res_model, "res.users")
        self.assertEqual(attachment.res_id, self.owner.id)
        # Served publicly, and the editor accepts exactly that address in a body.
        self.authenticate(None, None)
        self.assertEqual(self.url_open(ok["url"]).status_code, 200)
        self.authenticate("ownerone", "ownerone")
        self.assertIn(ok["url"], sanitize_member_html(f'<img src="{ok["url"]}" alt="x">')[0])
        # Not an image, SVG, too big, wrong CSRF.
        self.assertEqual(self._upload(b"not an image").status_code, 415)
        self.assertEqual(self._upload(b"<svg onload='x()'/>", "x.svg").status_code, 415)
        self.assertEqual(self._upload(PNG + b"\x00" * (owner_site.MAX_IMAGE_BYTES + 1)).status_code, 413)
        bad = self.url_open(
            "/my/site/ownerone/image",
            data={"json_reply": "1"},
            files={"ufile": ("p.png", PNG)},
            method="POST",
        )
        self.assertEqual(bad.status_code, 400)
        # Another member cannot upload into this site.
        self.authenticate("ownertwo", "ownertwo")
        self.assertEqual(self._upload(PNG).status_code, 404)

    def test_20_image_limit_per_site(self):
        self.authenticate("ownerone", "ownerone")
        attachments = self.env["ir.attachment"]
        for i in range(owner_site.MAX_IMAGES_PER_SITE):
            attachments |= attachments.create(
                {
                    "name": f"i{i}",
                    "datas": base64.b64encode(PNG),
                    "mimetype": "image/png",
                    "public": True,
                    "res_model": "res.users",
                    "res_id": self.owner.id,
                    "description": owner_site.IMAGE_DESCRIPTION,
                }
            )
        self.env.flush_all()
        self.assertEqual(self._upload(PNG).status_code, 409)

    def test_21_page_urls_stay_in_the_owners_namespace(self):
        # Tests [@ANCHOR: user_websites:COMM_owner_page_save]
        self.authenticate("ownerone", "ownerone")
        self._post(
            "/my/site/ownerone/page/save", title="Sneaky", page_slug="x/../../admin", content="<p>x</p>"
        )
        self.assertFalse(self.env["website.page"].search([("name", "=", "Sneaky")]))
        self.assertIsNone(re.match(PAGE_SLUG_RE, "x/y"))
