# This software is distributed under the terms of the Affero General Public License (AGPL-3).

# -*- coding: utf-8 -*-
import odoo.tests
from odoo.tests import tagged
from lxml import etree


@tagged("post_install", "-at_install")
class TestXPathRendering(odoo.tests.common.HttpCase):
    """
    ADR-0053: Exhaustive tests to mathematically prove that all XML XPath
    injections successfully render in the compiled architecture and browser DOM.
    """

    def setUp(self):
        super(TestXPathRendering, self).setUp()
        self.portal_user = self.env["res.users"].create(
            {
                "name": "Portal User",
                "login": "portaluser",
                "password": "portaluser",
                "email": "portal@example.com",
                "group_ids": [(6, 0, [self.env.ref("base.group_portal").id])],
            }
        )

    def test_01_res_config_settings(self):
        # [@ANCHOR: test_dropzone_settings]

        # Tests [@ANCHOR: dropzone_settings]
        res = self.env["res.config.settings"].get_view(
            view_id=self.env.ref("base.res_config_settings_view_form").id,
            view_type="form",
        )
        self.assertIn(
            'data-key="user_websites"',
            res["arch"],
            "The injected settings block must exist in the compiled arch.",
        )

    def test_02_res_users(self):
        # [@ANCHOR: test_dropzone_users]

        # Tests [@ANCHOR: dropzone_users]
        res = self.env["res.users"].get_view(
            view_id=self.env.ref("base.view_users_form").id, view_type="form"
        )
        self.assertIn(
            'name="user_websites_settings"',
            res["arch"],
            "The injected notebook page must exist in the compiled arch.",
        )

    def test_03_blog_post(self):
        # [@ANCHOR: test_dropzone_blog_post]

        # [@ANCHOR: test_xpath_rendering_blog_post]

        # Tests [@ANCHOR: dropzone_blog_post]

        # Tests [@ANCHOR: xpath_rendering_blog_post]
        res = self.env["blog.post"].get_view(
            view_id=self.env.ref("website_blog.view_blog_post_form").id,
            view_type="form",
        )
        self.assertIn(
            'name="user_websites_group_id"',
            res["arch"],
            "The injected proxy owner fields must exist in the compiled arch.",
        )

    def test_04_snippets(self):
        # [@ANCHOR: test_dropzone_snippets]

        # Tests [@ANCHOR: dropzone_snippets]
        # website.snippets is a QWeb view, so we pull its combined architecture
        view = self.env.ref("website.snippets")
        arch = view.with_context(lang=None)._get_combined_arch()
        arch_str = etree.tostring(arch, encoding="unicode")
        self.assertIn(
            'id="snippet_user_websites"',
            arch_str,
            "The snippet injection must successfully root into the parent view.",
        )

    def test_05_portal_templates(self):
        # [@ANCHOR: test_dropzone_templates]

        # [@ANCHOR: test_xpath_rendering_appeal]

        # [@ANCHOR: test_xpath_rendering_portal_docs]

        # Tests [@ANCHOR: dropzone_templates]

        # Tests [@ANCHOR: xpath_rendering_appeal]

        # Tests [@ANCHOR: xpath_rendering_portal_docs]
        self.authenticate(self.portal_user.login, self.portal_user.login)
        response = self.url_open("/my/home")
        self.assertEqual(response.status_code, 200)
        self.assertIn(b"Privacy", response.content)
        self.assertIn(b"Data", response.content)
        self.assertIn(b'id="user_websites_dropzone_templates"', response.content)

    def test_06_layout_templates(self):
        # [@ANCHOR: test_dropzone_layout]

        # [@ANCHOR: test_xpath_rendering_layout]

        # Tests [@ANCHOR: dropzone_layout]

        # Tests [@ANCHOR: xpath_rendering_layout]
        self.authenticate(None, None)
        response = self.url_open("/")
        self.assertEqual(response.status_code, 200)
        self.assertIn(
            b'id="reportViolationModal"',
            response.content,
            "The global website layout must render the injected reporting modal.",
        )

    def test_07_navbar_rendering(self):
        # [@ANCHOR: test_dropzone_navbar]

        # [@ANCHOR: test_xpath_rendering_navbar]

        # [@ANCHOR: test_xpath_rendering_navbar_head]

        # Tests [@ANCHOR: dropzone_navbar]

        # Tests [@ANCHOR: xpath_rendering_navbar]

        # Tests [@ANCHOR: xpath_rendering_navbar_head]

        # [@ANCHOR: test_dropzone_home_header]

        # [@ANCHOR: test_dropzone_home_footer]

        # [@ANCHOR: test_dropzone_navbar_actions]

        # Tests [@ANCHOR: dropzone_home_header]

        # Tests [@ANCHOR: dropzone_home_footer]

        # Tests [@ANCHOR: dropzone_navbar_actions]
        user = self.env["res.users"].create(
            {
                "name": "Nav User",
                "login": "navuser",
                "website_slug": "navuser",
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
        arch_string = f"""<t name="Home" t-name="user_websites.home_{user.website_slug}">
            <t t-call="user_websites.template_default_home">
                <div id="wrap" class="oe_structure oe_empty"/>
            </t>
        </t>"""

        self.env["website.page"].create(
            {
                "url": f"/{user.website_slug}/home",
                "name": "Home",
                "type": "qweb",
                "website_published": True,
                "is_published": True,
                "owner_user_id": user.id,
                "arch": arch_string,
            }
        )

        response = self.url_open(f"/{user.website_slug}/home")
        self.assertEqual(response.status_code, 200)
        self.assertIn(
            b'id="user_websites_dropzone_home_header"',
            response.content,
            "The home header dropzone must render.",
        )
        self.assertIn(
            b'id="user_websites_dropzone_home_footer"',
            response.content,
            "The home footer dropzone must render.",
        )
        self.assertIn(
            b'id="user_websites_dropzone_navbar_actions"',
            response.content,
            "The navbar actions dropzone must render.",
        )
        self.assertEqual(response.status_code, 200)
        self.assertIn(
            b'name="user_websites_slug"',
            response.content,
            "The user navbar context meta tag must render.",
        )
        self.assertIn(
            b'id="userNavbarNav"',
            response.content,
            "The dynamic user navigation bar must render on the page.",
        )

    def test_07b_navbar_member_owned_record_public_visitor(self):
        # [@ANCHOR: test_navbar_member_owned_record_public_visitor]

        # Tests [@ANCHOR: xpath_rendering_navbar_head]

        # Tests [@ANCHOR: mixin_navbar_profile_user]
        """A published record owned by a member must render for visitors who
        cannot read res.users: logged out, and logged in as another member.

        The navbar head resolves the owner from main_object.owner_user_id.
        website.page is served with a sudo'd main_object, so it never hit
        this; any controller passing the visitor's own record (website_blog's
        post page here, ham_events' event page in hams_com) got a 403 because
        the template read the owner's website_slug as the visitor.
        """
        owner, other = [
            self.env["res.users"].create(
                {
                    "name": f"Navbar {tag} Member",
                    "login": f"navbar-{tag}-member",
                    "password": f"navbar-{tag}-member",
                    "email": f"navbar-{tag}@example.com",
                    "website_slug": f"navbar-{tag}-member",
                    "group_ids": [
                        (
                            6,
                            0,
                            [
                                self.env.ref("base.group_portal").id,
                                self.env.ref(
                                    "user_websites.group_user_websites_user"
                                ).id,
                            ],
                        )
                    ],
                }
            )
            for tag in ("owner", "other")
        ]
        website = self.env["website"].get_current_website()
        blog = self.env["blog.blog"].create(
            {
                "name": "Navbar Owner Blog",
                "website_id": website.id,
                "owner_user_id": owner.id,
            }
        )
        post = self.env["blog.post"].create(
            {
                "name": "Navbar Owner Post",
                "blog_id": blog.id,
                "is_published": True,
                "website_id": website.id,
                "owner_user_id": owner.id,
            }
        )
        # The helper itself, called as a visitor who cannot read res.users.
        resolved = post.with_user(other)._user_websites_navbar_profile_user()
        self.assertEqual(resolved.id, owner.id)
        self.assertEqual(resolved.website_slug, "navbar-owner-member")
        unowned = self.env["blog.post"].new({"name": "Navbar Unowned Post"})
        self.assertFalse(unowned._user_websites_navbar_profile_user())

        url = post.website_url
        slug_meta = b'name="user_websites_slug" content="navbar-owner-member"'

        for login in (None, other.login):
            with self.subTest(visitor=login or "public"):
                self.authenticate(login, login)
                response = self.url_open(url)
                self.assertEqual(response.status_code, 200)
                self.assertIn(b"Navbar Owner Post", response.content)
                self.assertIn(slug_meta, response.content)
                self.assertIn(b'id="userNavbarNav"', response.content)
                self.assertIn(b"Navbar owner Member", response.content)

    def test_07c_report_violation_scoped_to_personal_website_content(self):
        # [@ANCHOR: test_report_violation_scoped_to_personal_website]

        # Tests [@ANCHOR: report_violation_scoped_to_personal_website_content]
        """The report-violation widget is meant for flagging an abusive personal operator
        website (blog.post here) -- it must render for a non-owner visitor, and must NOT
        render for the owner viewing their own page. Regression coverage for the fix itself:
        before it, ANY main_object carrying user_websites.owned.mixin (not just real
        personal-website content) rendered this widget, including models from other modules
        entirely unrelated to personal websites (event.event in hams_com, for one real
        example) -- this test only proves the ALLOWED case still works correctly; the
        excluded-model case can't be reproduced from hams_open alone, since every model here
        that uses the mixin genuinely is personal-website content.
        """
        owner, other = [
            self.env["res.users"].create(
                {
                    "name": f"Report Violation {tag} Member",
                    "login": f"report-violation-{tag}-member",
                    "password": f"report-violation-{tag}-member",
                    "email": f"report-violation-{tag}@example.com",
                    "website_slug": f"report-violation-{tag}-member",
                    "group_ids": [
                        (
                            6,
                            0,
                            [
                                self.env.ref("base.group_portal").id,
                                self.env.ref(
                                    "user_websites.group_user_websites_user"
                                ).id,
                            ],
                        )
                    ],
                }
            )
            for tag in ("owner", "other")
        ]
        website = self.env["website"].get_current_website()
        blog = self.env["blog.blog"].create(
            {
                "name": "Report Violation Owner Blog",
                "website_id": website.id,
                "owner_user_id": owner.id,
            }
        )
        post = self.env["blog.post"].create(
            {
                "name": "Report Violation Owner Post",
                "blog_id": blog.id,
                "is_published": True,
                "website_id": website.id,
                "owner_user_id": owner.id,
            }
        )
        url = post.website_url

        self.authenticate(other.login, other.login)
        response = self.url_open(url)
        self.assertEqual(response.status_code, 200)
        self.assertIn(
            b"user-websites-report-container",
            response.content,
            "A non-owner visitor must see the report-violation widget on real personal "
            "website content (a blog post).",
        )

        self.authenticate(owner.login, owner.login)
        owner_response = self.url_open(url)
        self.assertEqual(owner_response.status_code, 200)
        self.assertNotIn(
            b"user-websites-report-container",
            owner_response.content,
            "The owner viewing their own page must not see a widget for reporting "
            "themselves.",
        )

    def test_08_backend_views_rendering(self):
        # [@ANCHOR: test_user_websites_backend_views_rendering]
        """Verify that standard backend views compile without error."""
        v1 = self.env["content.violation.appeal"].get_view(
            view_id=self.env.ref("user_websites.view_content_violation_appeal_list").id,
            view_type="list",
        )
        self.assertIn("user_id", v1["arch"])

        v2 = self.env["content.violation.appeal"].get_view(
            view_id=self.env.ref("user_websites.view_content_violation_appeal_form").id,
            view_type="form",
        )
        self.assertIn("reason", v2["arch"])

        # content.violation.report's own views moved to content_moderation
        # on 2026-09-23 (that module now owns the model); get_view() still
        # returns the merged arch, including user_websites' own
        # content_group_id extension added via view inheritance
        # (views/content_violation_report_moderation_views.xml), so these
        # assertions are unchanged.
        v3 = self.env["content.violation.report"].get_view(
            view_id=self.env.ref(
                "content_moderation.view_content_violation_report_kanban"
            ).id,
            view_type="kanban",
        )
        self.assertIn("target_url", v3["arch"])

        v4 = self.env["content.violation.report"].get_view(
            view_id=self.env.ref("content_moderation.view_content_violation_report_list").id,
            view_type="list",
        )
        self.assertIn("content_owner_id", v4["arch"])

        v5 = self.env["content.violation.report"].get_view(
            view_id=self.env.ref("content_moderation.view_content_violation_report_form").id,
            view_type="form",
        )
        self.assertIn("reported_by_email", v5["arch"])

        v6 = self.env["user.websites.group"].get_view(
            view_id=self.env.ref("user_websites.view_user_websites_group_list").id,
            view_type="list",
        )
        self.assertIn("name", v6["arch"])

        v7 = self.env["user.websites.group"].get_view(
            view_id=self.env.ref("user_websites.view_user_websites_group_form").id,
            view_type="form",
        )
        self.assertIn("odoo_group_id", v7["arch"])

        v8 = self.env["website.page"].get_view(
            view_id=self.env.ref("user_websites.view_user_websites_page_list").id,
            view_type="list",
        )
        self.assertIn("website_published", v8["arch"])
