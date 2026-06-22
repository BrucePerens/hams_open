# -*- coding: utf-8 -*-
from odoo.tests.common import tagged
from odoo.addons.zero_sudo.tests.common import HamsHttpCase
import odoo.http


@tagged("post_install", "-at_install")
class TestManualFeatures(HamsHttpCase):

    def setUp(self):
        super(TestManualFeatures, self).setUp()

        self.searchable_article = self.env["knowledge.article"].create(
            {
                "name": "How to deploy Python",
                "body": "<p>This is a guide about deploying python web applications.</p>",
                "is_published": True,
            }
        )

        self.hidden_article = self.env["knowledge.article"].create(
            {
                "name": "Secret Python Configs",
                "body": "<p>Do not share these configs.</p>",
                "is_published": False,
            }
        )

    # Tests [@ANCHOR: story_manual_search]
    def test_01_search_functionality(self):
        """Verify the search route correctly identifies published content and hides unpublished."""
        self.authenticate(None, None)

        # Search by title keyword
        response = self.url_open("/manual/search?search=deploy")
        self.assertEqual(response.status_code, 200)
        self.assertIn(b"How to deploy Python", response.content)

        # Search by body keyword
        response_body = self.url_open("/manual/search?search=applications")
        self.assertIn(b"How to deploy Python", response_body.content)

        # Ensure hidden content is NOT returned in search results
        response_hidden = self.url_open("/manual/search?search=Secret")
        self.assertNotIn(
            b"Secret Python Configs",
            response_hidden.content,
            "Unpublished articles must never appear in public search results.",
        )

    def test_02_article_feedback_submission(self):
        # Tests [@ANCHOR: story_manual_feedback]
        """Verify the feedback controller securely increments the helpful counter via Service Account."""
        self.authenticate(None, None)

        self.assertEqual(self.searchable_article.helpful_count, 0)
        self.assertEqual(self.searchable_article.unhelpful_count, 0)

        # Submit a 'Helpful' rating
        response = self.url_open(
            "/manual/feedback",
            data={
                "csrf_token": odoo.http.Request.csrf_token(self),
                "article_id": self.searchable_article.id,
                "is_helpful": "1",
            },
            method="POST",
        )

        # Check that the database counter was incremented
        self.searchable_article.invalidate_recordset(["helpful_count"])
        self.assertEqual(
            self.searchable_article.helpful_count,
            1,
            "The helpful_count integer should be incremented.",
        )

        # Verify safe redirection
        self.assertIn(
            b"feedback_submitted=1",
            response.url.encode(),
            "The controller must append the success parameter and redirect safely.",
        )

    def test_03_negative_feedback_submission(self):
        """Verify the feedback controller securely increments the unhelpful counter via Service Account."""
        self.authenticate(None, None)

        self.assertEqual(self.searchable_article.unhelpful_count, 0)

        # Submit a 'Not Helpful' rating
        response = self.url_open(
            "/manual/feedback",
            data={
                "csrf_token": odoo.http.Request.csrf_token(self),
                "article_id": self.searchable_article.id,
                "is_helpful": "0",
            },
            method="POST",
        )
        self.assertEqual(response.status_code, 200)

        # Check that the database counter was incremented
        self.searchable_article.invalidate_recordset(["unhelpful_count"])
        self.assertEqual(
            self.searchable_article.unhelpful_count,
            1,
            "The unhelpful_count integer should be incremented.",
        )

    def test_04_doc_installation(self):

        # Tests [@ANCHOR: manual_doc_injection]
        # Tests [@ANCHOR: manual_doc_auto_install]
        """Verify that documentation from the manifest is correctly installed."""
        # Trigger bootstrap manually to ensure it runs during the test
        self.env['ir.module.module']._bootstrap_knowledge_docs()

        article = self.env["knowledge.article"].search([("name", "=", "Knowledge: User Guide")])
        self.assertTrue(article.exists(), "User Guide article should have been installed.")
        self.assertIn("Welcome to the Knowledge", article.body)
        self.assertTrue(article.is_published)
