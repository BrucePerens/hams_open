# This software is distributed under the terms of the Affero General Public License (AGPL-3).

# -*- coding: utf-8 -*-
from odoo.tests.common import tagged
from odoo.addons.zero_sudo.tests.common import HamsHttpCase


@tagged("post_install", "-at_install")
class TestKnowledgeArticleWebsiteSearch(HamsHttpCase):
    def setUp(self):
        super().setUp()
        self.published_article = self.env["knowledge.article"].create(
            {
                "name": "Xyzzyplugh Interfacing Guide",
                "body": "<p>Wire a Xyzzyplugh cable to your radio's data port.</p>",
                "is_published": True,
                "internal_permission": "read",
            }
        )
        self.unpublished_article = self.env["knowledge.article"].create(
            {
                "name": "Xyzzyplugh Internal Notes",
                "body": "<p>Xyzzyplugh vendor pricing, not for public eyes.</p>",
                "is_published": False,
                "internal_permission": "read",
            }
        )

    def test_01_search_finds_a_published_article_by_title(self):
        # Found live 2026-08-29 across three separate hams_com usability-audit
        # personas: the site's own search never covered knowledge articles at
        # all, only products/blog/pages -- a genuinely public, published
        # manual article was completely unfindable through search. This
        # verifies the fix (knowledge.article now registers itself with
        # website's search dispatch) actually works end to end through the
        # real /website/search route, not just at the model level.
        response = self.url_open("/website/search?search=Xyzzyplugh")
        self.assertEqual(response.status_code, 200)
        # Not a literal "Xyzzyplugh Interfacing Guide" substring check: the
        # matched search term is wrapped in its own highlight span by
        # website's own autocomplete() rendering, splitting the title's raw
        # text across tags. Check the two halves separately instead.
        self.assertIn(b"Xyzzyplugh", response.content)
        self.assertIn(b"Interfacing Guide", response.content)

    def test_02_search_does_not_leak_an_unpublished_article(self):
        response = self.url_open("/website/search?search=Xyzzyplugh")
        self.assertEqual(response.status_code, 200)
        self.assertNotIn(b"Xyzzyplugh Internal Notes", response.content)
