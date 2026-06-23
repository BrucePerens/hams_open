# -*- coding: utf-8 -*-
import odoo.tests
from odoo.tests import tagged
from lxml import etree


@tagged('post_install', '-at_install')
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
            "The home header dropzone must render."
        )
        self.assertIn(
            b'id="user_websites_dropzone_home_footer"',
            response.content,
            "The home footer dropzone must render."
        )
        self.assertIn(
            b'id="user_websites_dropzone_navbar_actions"',
            response.content,
            "The navbar actions dropzone must render."
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

        v3 = self.env["content.violation.report"].get_view(
            view_id=self.env.ref(
                "user_websites.view_content_violation_report_kanban"
            ).id,
            view_type="kanban",
        )
        self.assertIn("target_url", v3["arch"])

        v4 = self.env["content.violation.report"].get_view(
            view_id=self.env.ref("user_websites.view_content_violation_report_list").id,
            view_type="list",
        )
        self.assertIn("content_owner_id", v4["arch"])

        v5 = self.env["content.violation.report"].get_view(
            view_id=self.env.ref("user_websites.view_content_violation_report_form").id,
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
