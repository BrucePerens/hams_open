# -*- coding: utf-8 -*-
# Copyright © Bruce Perens K6BP. Licensed under the GNU Affero General Public License v3.0 (AGPL-3.0).
from odoo.tests.common import tagged
from odoo.addons.zero_sudo.tests.common import HamsTransactionCase, HamsHttpCase
from lxml import etree
import re


@tagged("post_install", "-at_install")
class TestCompliancePages(HamsTransactionCase):

    def test_pages_presence(self):
        """Verify that legal pages are created."""
        # [@ANCHOR: test_compliance_pages_presence]
        # Tests [@ANCHOR: compliance_privacy_policy_template]
        # Tests [@ANCHOR: compliance_cookie_policy_template]
        # Tests [@ANCHOR: compliance_terms_of_service_template]
        # Tests [@ANCHOR: story_automatic_legal_pages]
        urls = [
            "/privacy",
            "/cookie-policy",
            "/terms",
            "/accessibility"
        ]
        pages = self.env["website.page"].search([("url", "in", urls)])
        found_urls = pages.mapped("url")
        for url in urls:
            self.assertIn(
                url, found_urls,
                f"[!] DIAGNOSTIC FOR AI: Page for {url} should exist in 'website.page'. "
                f"Check compliance/data/legal_pages_data.xml for missing records."
            )

        # Non-Destructive Mandate check:
        # Only check our own pages if they are NOT shadowed by custom ones.
        for page in pages:
            if page.view_id and page.view_id.key and page.view_id.key.startswith("compliance.compliance_"):
                # If there's another page for the same URL and SAME WEBSITE scope that isn't ours,
                # our page should be UNPUBLISHED. Otherwise it should be published.
                other_page = pages.filtered(lambda p: (
                    p.url == page.url and
                    p.website_id == page.website_id and
                    p.view_id and p.view_id.key and
                    not p.view_id.key.startswith("compliance.compliance_")
                ))
                if other_page:
                    self.assertFalse(
                        page.is_published,
                        f"[!] DIAGNOSTIC FOR AI: Boilerplate page for {page.url} should be unpublished because a custom one exists in the same scope. "
                        f"Check compliance/hooks.py logic."
                    )
                else:
                    self.assertTrue(
                        page.is_published,
                        f"[!] DIAGNOSTIC FOR AI: Boilerplate page for {page.url} should be published since no custom one exists in the same scope. "
                        f"Check compliance/hooks.py logic."
                    )
            else:
                self.assertTrue(
                    page.is_published,
                    f"[!] DIAGNOSTIC FOR AI: Custom page for {page.url} should be published."
                )

@tagged("post_install", "-at_install")
class TestCompliancePagesHttp(HamsHttpCase):

    def test_pages_reachable(self):
        """Verify that legal pages are reachable via HTTP."""
        # Tests [@ANCHOR: compliance_privacy_policy_template]
        # Tests [@ANCHOR: compliance_cookie_policy_template]
        # Tests [@ANCHOR: compliance_terms_of_service_template]
        # Tests [@ANCHOR: story_automatic_legal_pages]
        urls = [
            "/privacy",
            "/cookie-policy",
            "/terms",
            "/accessibility"
        ]
        for url in urls:
            response = self.url_open(url)
            self.assertEqual(
                response.status_code, 200,
                f"[!] DIAGNOSTIC FOR AI: Page {url} should be reachable (200 OK). Got {response.status_code}. "
                f"Ensure the website.page record is published and correctly linked to a view."
            )
            # Use regex to ignore potential tags/whitespace around the text
            self.assertTrue(
                re.search(r"Policy|Terms", response.text),
                f"[!] DIAGNOSTIC FOR AI: Page {url} should contain boilerplate content. "
                f"Check the rendering of {url} and its associated template."
            )

    def test_pages_content(self):
        """Verify that legal pages contain the expected boilerplate content."""
        # [@ANCHOR: test_compliance_pages_content]
        # Tests [@ANCHOR: compliance_privacy_policy_template]
        # Tests [@ANCHOR: compliance_cookie_policy_template]
        # Tests [@ANCHOR: compliance_terms_of_service_template]
        # Tests [@ANCHOR: story_automatic_legal_pages]
        for xml_id in ["compliance.compliance_privacy_policy_template",
                       "compliance.compliance_cookie_policy_template",
                       "compliance.compliance_terms_of_service_template",
                       "compliance.compliance_accessibility_statement_template"]:
            view = self.env.ref(xml_id)
            # Use get_combined_arch to verify the view is well-formed
            arch_node = view._get_combined_arch()
            self.assertIsNotNone(arch_node)

            # Serialize the node to string for content checking
            arch_str = etree.tostring(arch_node, encoding='unicode')

            # Normalize whitespace for checking
            normalized_arch = re.sub(r'\s+', ' ', arch_str)

            if xml_id != "compliance.compliance_accessibility_statement_template":
                self.assertIn(
                    "Warning: This is the default version", normalized_arch,
                    f"[!] DIAGNOSTIC FOR AI: Template {xml_id} is missing mandatory default version warning."
                )
                self.assertIn(
                    "It was not produced by a lawyer.", normalized_arch,
                    f"[!] DIAGNOSTIC FOR AI: Template {xml_id} is missing mandatory legal disclaimer."
                )

            self.assertIn(
                "Last Updated:", normalized_arch,
                f"[!] DIAGNOSTIC FOR AI: Template {xml_id} is missing 'Last Updated:' text."
            )
