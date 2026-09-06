# This software is distributed under the terms of the Affero General Public License (AGPL-3).

# -*- coding: utf-8 -*-
from odoo.tests.common import tagged
from odoo.addons.zero_sudo.tests.common import HamsTransactionCase


@tagged("post_install", "-at_install")
class TestManualUIEnhancements(HamsTransactionCase):

    def test_01_reading_time_calculation(self):
        # [@ANCHOR: test_manual_reading_time]

        # Tests [@ANCHOR: manual_compute_reading_time]
        """Verify that reading time is calculated correctly based on word count."""
        # ~200 words = 1 minute
        body_content = "<p>" + "word " * 200 + "</p>"
        article = self.env["knowledge.article"].create(
            {
                "name": "Reading Time Test",
                "body": body_content,
            }
        )
        self.assertEqual(
            article.reading_time,
            1,
            "[!] DIAGNOSTIC FOR AI: Reading time for 200 words should be 1 minute.",
        )

        # ~400 words = 2 minutes
        article.body = "<p>" + "word " * 400 + "</p>"
        self.assertEqual(
            article.reading_time,
            2,
            "[!] DIAGNOSTIC FOR AI: Reading time for 400 words should be 2 minutes.",
        )

        # Empty body = 0 minutes
        article.body = False
        self.assertEqual(
            article.reading_time,
            0,
            "[!] DIAGNOSTIC FOR AI: Reading time for empty body should be 0 minutes.",
        )

    def test_01b_body_snippet_is_a_truncated_plain_text_preview(self):
        # Tests [@ANCHOR: knowledge:COMM_compute_body_snippet]
        long_word_run = "word " * 100
        article = self.env["knowledge.article"].create(
            {
                "name": "Body Snippet Test",
                "body": f"<p>{long_word_run}</p>",
            }
        )
        self.assertNotIn("<", article.body_snippet)
        self.assertLessEqual(len(article.body_snippet), 300)
        self.assertTrue(article.body_snippet.startswith("word word word"))

        article.body = False
        self.assertEqual(article.body_snippet, "")

    def test_02_ui_enhancements_rendering(self):
        # [@ANCHOR: test_manual_ui_rendering]

        # Tests [@ANCHOR: knowledge:COMM_compute_author_id]
        """Verify that the new UI elements are present in the rendered template."""
        article = self.env["knowledge.article"].create(
            {
                "name": "UI Rendering Test",
                "body": "<p>Some content</p>",
                "is_published": True,
            }
        )

        # Use the test runner's HttpCase to render the page if needed,
        # but here we can check the computed fields which are then used in the template.
        self.assertTrue(article.reading_time >= 0)
        self.assertTrue(article.write_date)
        self.assertTrue(article.author_id)
        self.assertEqual(article.author_id, self.env.user)

    def test_03_copy_article(self):
        # Tests [@ANCHOR: knowledge:COMM_copy]
        """Verify that copying an article preserves hierarchy and updates name."""
        parent = self.env["knowledge.article"].create({"name": "Parent"})
        child = self.env["knowledge.article"].create(
            {
                "name": "Child",
                "parent_id": parent.id,
            }
        )
        child_copy = child.copy()
        self.assertEqual(child_copy.parent_id, parent)
        self.assertIn("(copy)", child_copy.name)
