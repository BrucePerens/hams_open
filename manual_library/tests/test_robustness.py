# -*- coding: utf-8 -*-
import odoo.tests
import urllib.parse


@odoo.tests.common.tagged("post_install", "-at_install")
class TestManualRobustness(odoo.tests.common.HttpCase):

    def setUp(self):
        super().setUp()
        self.article_active = self.env["knowledge.article"].create(
            {
                "name": "Active Article",
                "body": "<p>Active</p>",
                "is_published": True,
                "active": True,
            }
        )
        self.article_archived = self.env["knowledge.article"].create(
            {
                "name": "Archived Article",
                "body": "<p>Archived</p>",
                "is_published": True,
                "active": False,
            }
        )

    def test_01_archived_articles_hidden(self):
        """Verify that standard active=False archiving correctly hides the article from public routes."""
        self.authenticate(None, None)

        # Access base route (Checking the dynamic sidebar generation)
        response = self.url_open("/manual")
        self.assertIn(b"Active Article", response.content)
        self.assertNotIn(
            b"Archived Article",
            response.content,
            "Archived articles must be hidden from the sidebar.",
        )

        # Direct Access should 404
        response_direct = self.url_open(self.article_archived.website_url)
        self.assertEqual(
            response_direct.status_code, 404, "Archived articles must return a 404."
        )

        # Search should omit archived
        response_search = self.url_open("/manual/search?search=Archived")
        self.assertNotIn(
            b"Archived Article",
            response_search.content,
            "Search must filter out inactive articles.",
        )

    def test_02_xss_in_search_input(self):
        """Verify that malicious XSS input in the search field is safely escaped in the rendered view."""
        self.authenticate(None, None)

        malicious_string = "<script>alert('XSS')</script>"
        safe_url = "/manual/search?search=" + urllib.parse.quote(malicious_string)
        response = self.url_open(safe_url)

        self.assertEqual(response.status_code, 200)
        self.assertNotIn(
            b"<script>alert('XSS')</script>",
            response.content,
            "The raw malicious string must not be rendered.",
        )
        self.assertIn(
            b"&lt;script&gt;alert(&#39;XSS&#39;)&lt;/script&gt;",
            response.content,
            "The search term must be safely escaped via t-out.",
        )

    def test_03_active_child_of_archived_parent(self):
        """
        Verify that an active child article whose parent is archived
        gracefully degrades and does not crash the dynamic sidebar generation.
        """
        child_article = self.env["knowledge.article"].create(
            {
                "name": "Active Child",
                "body": "<p>Child Body</p>",
                "is_published": True,
                "parent_id": self.article_archived.id,
            }
        )

        self.authenticate(None, None)

        # Accessing the child directly should work (it is individually active)
        response = self.url_open(child_article.website_url)
        self.assertEqual(
            response.status_code,
            200,
            "Active child should render successfully despite parent state.",
        )
        self.assertIn(b"Active Child", response.content)

        # The archived parent MUST NOT appear anywhere in the sidebar or breadcrumbs
        self.assertNotIn(
            b"Archived Article",
            response.content,
            "Archived parent must remain hidden from the UI hierarchy.",
        )

    def test_04_backend_views_rendering(self):
        # [@ANCHOR: test_manual_backend_views_rendering]
        # Tests [@ANCHOR: story_manual_backend_views]
        v1 = self.env["knowledge.article"].get_view(
            view_id=self.env.ref("manual_library.view_knowledge_article_list").id,
            view_type="list",
        )
        self.assertIn("parent_id", v1["arch"])
        v2 = self.env["knowledge.article"].get_view(
            view_id=self.env.ref("manual_library.view_knowledge_article_form").id,
            view_type="form",
        )
        self.assertIn("internal_permission", v2["arch"])

    def test_05_canonical_redirect(self):
        """Verify that accessing an article with an old slug redirects to the current one."""
        article = self.env["knowledge.article"].create({
            "name": "Canonical Test",
            "is_published": True,
        })
        old_slug = f"{article.id}-old-name"
        url = f"/manual/{old_slug}"

        response = self.url_open(url)
        self.assertEqual(response.status_code, 200)
        self.assertTrue(response.url.endswith(article.website_url))

    def test_06_honeypot_blocking(self):
        """Verify that the honeypot field blocks feedback submission."""
        article = self.article_active
        initial_helpful = article.helpful_count

        self.authenticate(None, None)
        res = self.url_open("/manual")
        csrf_anchor = '<input type="hidden" name="csrf_token" value="'
        csrf_token = res.text.partition(csrf_anchor)[2].partition('"')[0]

        self.url_open(
            "/manual/feedback",
            data={
                "csrf_token": csrf_token,
                "article_id": article.id,
                "is_helpful": "1",
                "website_feedback_honeypot": "i-am-a-bot",
            },
            method="POST",
        )

        article.invalidate_recordset(["helpful_count"])
        self.assertEqual(article.helpful_count, initial_helpful, "Honeypot should have blocked the increment.")

    def test_07_portal_access(self):
        """Verify that portal users can see published articles but not internal ones."""
        published_article = self.article_active
        self.env["res.users"].create({
            "name": "Portal User",
            "login": "portal_user_robust",
            "password": "portal_user_robust",
            "email": "portal_robust@test.com",
            "group_ids": [(6, 0, [self.env.ref("base.group_portal").id])],
        })

        internal_article = self.env["knowledge.article"].create({
            "name": "Internal Only Robust",
            "internal_permission": "read",
            "is_published": False,
        })

        self.authenticate("portal_user_robust", "portal_user_robust")

        # Can see published
        response = self.url_open(published_article.website_url)
        self.assertEqual(response.status_code, 200)

        # Cannot see internal
        response = self.url_open(internal_article.website_url)
        self.assertEqual(response.status_code, 404)
