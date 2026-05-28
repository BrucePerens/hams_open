# -*- coding: utf-8 -*-
from odoo.tests.common import tagged
from odoo.addons.zero_sudo.tests.common import HamsTransactionCase

@tagged("post_install", "-at_install")
class TestDocumentation(HamsTransactionCase):

    def test_documentation_installed(self):
        # Tests [@ANCHOR: caching_docs_bootstrap]
        """Verify that the caching documentation is correctly installed."""
        article_model = None
        if "knowledge.article" in self.env:
            article_model = "knowledge.article"
        elif "manual.article" in self.env:
            article_model = "manual.article"

        if not article_model:
            self.skipTest("Neither knowledge.article nor manual.article model available")

        article = self.env[article_model].search([("name", "=", "Caching Module Documentation")], limit=1)
        self.assertTrue(article, f"Caching Module Documentation should be created in {article_model}.")
        self.assertIn("Caching Module", article.body)
        self.assertEqual(article.icon, "⚡")
