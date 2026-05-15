# -*- coding: utf-8 -*-
import odoo.tests


@odoo.tests.common.tagged("post_install", "-at_install")
class TestManualLibraryUITours(odoo.tests.HttpCase):
    def test_01_manual_toc_tour(self):
        # [@ANCHOR: test_tour_manual_toc]
        # Tests [@ANCHOR: story_manual_toc]
        # Create an article with the appropriate HTML headings to trigger the TOC generator
        article = self.env["knowledge.article"].create(
            {
                "name": "TOC Test Article",
                "body": "<h2>Section 1</h2><p>Content</p><h3>Subsection</h3>",
                "is_published": True,
            }
        )
        # Start the frontend tour
        self.start_tour(article.website_url, "manual_toc_tour")

    def test_02_manual_search_tour(self):
        # [@ANCHOR: test_tour_manual_search]
        self.env["knowledge.article"].create(
            {
                "name": "Odoo Search Test",
                "body": "<p>Testing the search.</p>",
                "is_published": True,
            }
        )
        self.start_tour("/manual", "manual_search_tour")

    def test_03_manual_feedback_tour(self):
        # [@ANCHOR: test_tour_manual_feedback]
        article = self.env["knowledge.article"].create(
            {
                "name": "Feedback Test",
                "body": "<p>Testing feedback.</p>",
                "is_published": True,
            }
        )
        self.start_tour(article.website_url, "manual_feedback_tour")
