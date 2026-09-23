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

    def test_01c_body_snippet_never_leaks_a_multiline_html_comment_or_pseudo_markdown(self):
        # Tests [@ANCHOR: knowledge:COMM_compute_body_snippet]
        # Real bug found doing overnight usability testing, 2026-09-22: at least three real
        # knowledge articles' search-result snippets showed a full internal maintainer-only
        # review-status HTML comment, plus "**"/"/.../ " decoration for headings/emphasis,
        # both visible to any reader on a public search page. Traced into Odoo core's own
        # html2plaintext(): it deliberately renders <h1>/<em> as "**...**"/"/.../ " (a real
        # feature for HTML-email plaintext fallbacks, not a bug on its own), and its final
        # tag-stripping regex (`re.sub('<.*?>', ' ', html)`) has no re.DOTALL, so it can
        # never fully match an HTML comment whose own content spans more than one line --
        # exactly what a real multi-line maintainer comment looks like. This article
        # reproduces the real shape (a heading, an <em> byline, then a two-line comment)
        # that the actual affected articles had, confirmed directly against their real
        # stored `body` content before this fix.
        article = self.env["knowledge.article"].create(
            {
                "name": "Multiline Comment Leak Test",
                "body": (
                    "<h1>Using This Software</h1>\n"
                    "<!-- [@ANCHOR: doc_inject_example] -->\n"
                    '<p class="text-muted small"><em>Copyright hams.com. All Rights Reserved.</em></p>\n'
                    "<!-- Review status: drafted 2026-09-01, not yet reviewed by the maintainer.\n"
                    "Checked against how this module actually behaves directly. -->\n\n"
                    "<h2>What this covers</h2>\n"
                    "<p>Some real reader-facing content.</p>"
                ),
            }
        )
        self.assertNotIn("Review status", article.body_snippet)
        self.assertNotIn("<!--", article.body_snippet)
        self.assertNotIn("-->", article.body_snippet)
        self.assertNotIn("**", article.body_snippet)
        self.assertNotIn("/Copyright", article.body_snippet)
        self.assertIn("Using This Software", article.body_snippet)
        self.assertIn("Copyright hams.com", article.body_snippet)

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
