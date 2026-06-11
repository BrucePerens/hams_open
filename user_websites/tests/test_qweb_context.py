# -*- coding: utf-8 -*-
from odoo.tests import tagged
from odoo.addons.zero_sudo.tests.real_transaction import RealTransactionCase


@tagged("post_install", "-at_install")
class TestQWebContext(RealTransactionCase):
    """
    Tests focused on ensuring the controllers inject the correct context variables
    into QWeb templates to prevent KeyErrors and rendering crashes.
    """

    def setUp(self):
        super(TestQWebContext, self).setUp()

        # Setup Website
        self.website = self.env["website"].get_current_website()
        if not self.website:
            self.website = self.env["website"].search([], limit=1)

        # Setup User
        self.user_render = self.env["res.users"].create(
            {
                "name": "Render Tester",
                "login": "rendertester",
                "email": "render@example.com",
                "website_slug": "rendertester",
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

        # Setup Group
        self.group_render = self.env["user.websites.group"].create(
            {
                "name": "Render Group",
                "website_slug": "render-group",
                "member_ids": [(4, self.user_render.id)],
            }
        )

        # Create a blog post so the loop executes in QWeb
        self.blog = self.env["blog.blog"].search(
            [("name", "=", "Community Blog")], limit=1
        )
        if not self.blog:
            self.blog = self.env["blog.blog"].create(
                {"name": "Community Blog", "website_id": self.website.id}
            )

        self.env["blog.post"].create(
            {
                "name": "Context Test Post",
                "blog_id": self.blog.id,
                "owner_user_id": self.user_render.id,
                "is_published": True,
            }
        )

        user_arch = f"""<t name="User Home" t-name="user_websites.home_{self.user_render.website_slug}">
            <t t-call="user_websites.template_default_home">
                <div id="wrap" class="oe_structure oe_empty"/>
            </t>
        </t>"""

        group_arch = f"""<t name="Group Home" t-name="user_websites.home_{self.group_render.website_slug}">
            <t t-call="user_websites.template_default_home">
                <div id="wrap" class="oe_structure oe_empty"/>
            </t>
        </t>"""

        # Create Pages
        self.env["website.page"].create(
            {
                "url": f"/{self.user_render.website_slug}/home",
                "name": "User Home",
                "type": "qweb",
                "owner_user_id": self.user_render.id,
                "website_published": True,
                "arch": user_arch,
            }
        )

        self.env["website.page"].create(
            {
                "url": f"/{self.group_render.website_slug}/home",
                "name": "Group Home",
                "type": "qweb",
                "user_websites_group_id": self.group_render.id,
                "website_published": True,
                "arch": group_arch,
            }
        )

    def test_01_blog_rendering_context(self):
        """
        Ensure that the /blog route injects 'pager', 'blogs', 'main_object',
        and 'blog_url' into the context so standard Odoo templates don't crash.
        """
        response = self.url_open(f"/{self.user_render.website_slug}/blog")

        # A 500 error here usually means a KeyError in QWeb (e.g., missing pager)
        self.assertEqual(
            response.status_code,
            200,
            "The blog route must render successfully without QWeb KeyErrors.",
        )
        self.assertIn(
            b"Context Test Post",
            response.content,
            "The post content should be successfully rendered.",
        )

    def test_02_personal_home_rendering_context(self):
        """
        Verify that the personal homepage properly injects 'main_object' and 'is_owner'
        to ensure standard website blocks and conditional logic function correctly.
        """
        self.authenticate(self.user_render.login, self.user_render.login)
        response = self.url_open(f"/{self.user_render.website_slug}/home")

        self.assertEqual(
            response.status_code, 200, "The user homepage must render successfully."
        )

        # Verify the custom layout logic didn't break
        self.assertNotIn(
            b"Report Violation",
            response.content,
            "Since the user is the owner, the QWeb logic should hide the report button.",
        )

    def test_03_group_home_rendering_context(self):
        """
        Verify that the group homepage injects 'main_object' and identifies group members
        correctly in the QWeb rendering dictionary.
        """
        self.authenticate(self.user_render.login, self.user_render.login)
        response = self.url_open(f"/{self.group_render.website_slug}/home")

        self.assertEqual(
            response.status_code, 200, "The group homepage must render successfully."
        )

    def test_04_public_visitor_context_isolation(self):
        """
        Verify that an unauthenticated user rendering the page triggers the correct
        guest-facing layout components.
        """
        self.authenticate(None, None)
        response = self.url_open(f"/{self.user_render.website_slug}/home")

        self.assertEqual(response.status_code, 200)
        self.assertIn(
            b"Report Violation",
            response.content,
            "Public guests should successfully render the layout with the report button injected.",
        )

    def test_05_meta_slug_context_provider(self):
        """
        Verify that the universal context provider meta tag is injected into the head
        for downstream JS widgets to consume safely.
        """
        response = self.url_open(f"/{self.user_render.website_slug}/home")
        self.assertEqual(response.status_code, 200)

        self.assertIn(
            b'name="user_websites_slug"',
            response.content,
            "The layout MUST inject the user_websites_slug meta tag into the DOM.",
        )
        self.assertIn(
            f'content="{self.user_render.website_slug}"'.encode("utf-8"),
            response.content,
            "The meta tag MUST contain the correct resolved_slug.",
        )
