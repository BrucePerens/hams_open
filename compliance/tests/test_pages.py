# -*- coding: utf-8 -*-
# Copyright © Bruce Perens K6BP. Licensed under the GNU Affero General
# Public License v3.0 (AGPL-3.0).
from odoo.tests.common import tagged
from odoo.addons.zero_sudo.tests.common import HamsTransactionCase, HamsHttpCase
from lxml import etree
import re


@tagged("post_install", "-at_install")
class TestCompliancePages(HamsTransactionCase):

    def test_pages_presence(self):
        """Verify that legal pages are created."""
        # [@ANCHOR: COMM_test_compliance_pages_presence]

        # Tests [@ANCHOR: COMM_compliance_privacy_policy_template]

        # Tests [@ANCHOR: COMM_compliance_cookie_policy_template]

        # Tests [@ANCHOR: COMM_compliance_terms_of_service_template]

        # Tests [@ANCHOR: COMM_story_automatic_legal_pages]
        urls = ["/privacy", "/cookie-policy", "/terms", "/accessibility"]
        pages = self.env["website.page"].search([("url", "in", urls)], limit=1000)
        found_urls = pages.mapped("url")
        for url in urls:
            self.assertIn(
                url,
                found_urls,
                f"[!] DIAGNOSTIC FOR AI: Page for {url} should exist in 'website.page'. "
                "Check compliance/data/legal_pages_data.xml for missing records.",
            )

        # Non-Destructive Mandate check:
        # Only check our own pages if they are NOT shadowed by custom ones.
        for page in pages:
            if (
                page.view_id
                and page.view_id.key
                and page.view_id.key.startswith("compliance.compliance_")
            ):
                # If there's another page for the same URL and SAME WEBSITE scope that isn't ours,
                # our page should be UNPUBLISHED. Otherwise it should be
                # published.
                other_page = pages.filtered(
                    lambda p: (
                        p.url == page.url
                        and p.website_id == page.website_id
                        and p.view_id
                        and p.view_id.key
                        and not p.view_id.key.startswith("compliance.compliance_")
                    )
                )
                if other_page:
                    self.assertFalse(
                        page.is_published,
                        f"[!] DIAGNOSTIC FOR AI: Boilerplate page for {
                            page.url} should be unpublished because a custom one exists in the same scope. " "Check compliance/hooks.py logic.",
                    )
                else:
                    self.assertTrue(
                        page.is_published,
                        f"[!] DIAGNOSTIC FOR AI: Boilerplate page for {
                            page.url} should be published since no custom one exists in the same scope. " "Check compliance/hooks.py logic.",
                    )
            else:
                self.assertTrue(
                    page.is_published, f"[!] DIAGNOSTIC FOR AI: Custom page for {
                        page.url} should be published.", )


@tagged("post_install", "-at_install")
class TestCompliancePagesHttp(HamsHttpCase):

    def test_pages_reachable(self):
        """Verify that legal pages are reachable via HTTP."""
        # Tests [@ANCHOR: COMM_compliance_privacy_policy_template]

        # Tests [@ANCHOR: COMM_compliance_cookie_policy_template]

        # Tests [@ANCHOR: COMM_compliance_terms_of_service_template]

        # Tests [@ANCHOR: COMM_story_automatic_legal_pages]
        response = self.url_open("/privacy")
        self.assertEqual(
            response.status_code,
            200,
            f"[!] DIAGNOSTIC FOR AI: Page /privacy should be reachable (200 OK). Got {response.status_code}. "
            "Ensure the website.page record is published and correctly linked to a view.",
        )
        self.assertTrue(
            re.search(r"Policy|Terms", response.text),
            "[!] DIAGNOSTIC FOR AI: Page /privacy should contain boilerplate content. "
            "Check the rendering of /privacy and its associated template.",
        )

        response = self.url_open("/cookie-policy")
        self.assertEqual(
            response.status_code,
            200,
            f"[!] DIAGNOSTIC FOR AI: Page /cookie-policy should be reachable (200 OK). Got {response.status_code}. "
            "Ensure the website.page record is published and correctly linked to a view.",
        )
        self.assertTrue(
            re.search(r"Policy|Terms", response.text),
            "[!] DIAGNOSTIC FOR AI: Page /cookie-policy should contain boilerplate content. "
            "Check the rendering of /cookie-policy and its associated template.",
        )

        response = self.url_open("/terms")
        self.assertEqual(
            response.status_code,
            200,
            f"[!] DIAGNOSTIC FOR AI: Page /terms should be reachable (200 OK). Got {response.status_code}. "
            "Ensure the website.page record is published and correctly linked to a view.",
        )
        self.assertTrue(
            re.search(r"Policy|Terms", response.text),
            "[!] DIAGNOSTIC FOR AI: Page /terms should contain boilerplate content. "
            "Check the rendering of /terms and its associated template.",
        )

        response = self.url_open("/accessibility")
        self.assertEqual(
            response.status_code,
            200,
            f"[!] DIAGNOSTIC FOR AI: Page /accessibility should be reachable (200 OK). Got {response.status_code}. "
            "Ensure the website.page record is published and correctly linked to a view.",
        )
        self.assertTrue(
            re.search(r"Policy|Terms", response.text),
            "[!] DIAGNOSTIC FOR AI: Page /accessibility should contain boilerplate content. "
            "Check the rendering of /accessibility and its associated template.",
        )

    def test_pages_content(self):
        """Verify that legal pages contain the expected boilerplate content."""
        # [@ANCHOR: COMM_test_compliance_pages_content]

        # Tests [@ANCHOR: COMM_compliance_privacy_policy_template]

        # Tests [@ANCHOR: COMM_compliance_cookie_policy_template]

        # Tests [@ANCHOR: COMM_compliance_terms_of_service_template]

        # Tests [@ANCHOR: COMM_story_automatic_legal_pages]
        # /privacy
        view = self.env.ref("compliance.compliance_privacy_policy_template")
        arch_node = view._get_combined_arch()
        self.assertIsNotNone(arch_node)
        arch_str = etree.tostring(arch_node, encoding="unicode")
        normalized_arch = re.sub(r"\s+", " ", arch_str)
        self.assertIn(
            "Warning: This is the default version",
            normalized_arch,
            "[!] DIAGNOSTIC FOR AI: Template compliance.compliance_privacy_policy_template is missing mandatory default version warning.",
        )
        self.assertIn(
            "It was not produced by a lawyer.",
            normalized_arch,
            "[!] DIAGNOSTIC FOR AI: Template compliance.compliance_privacy_policy_template is missing mandatory legal disclaimer.",
        )
        self.assertIn(
            "Last Updated:",
            normalized_arch,
            "[!] DIAGNOSTIC FOR AI: Template compliance.compliance_privacy_policy_template is missing 'Last Updated:' text.",
        )

        # /cookie-policy
        view = self.env.ref("compliance.compliance_cookie_policy_template")
        arch_node = view._get_combined_arch()
        self.assertIsNotNone(arch_node)
        arch_str = etree.tostring(arch_node, encoding="unicode")
        normalized_arch = re.sub(r"\s+", " ", arch_str)
        self.assertIn(
            "Warning: This is the default version",
            normalized_arch,
            "[!] DIAGNOSTIC FOR AI: Template compliance.compliance_cookie_policy_template is missing mandatory default version warning.",
        )
        self.assertIn(
            "It was not produced by a lawyer.",
            normalized_arch,
            "[!] DIAGNOSTIC FOR AI: Template compliance.compliance_cookie_policy_template is missing mandatory legal disclaimer.",
        )
        self.assertIn(
            "Last Updated:",
            normalized_arch,
            "[!] DIAGNOSTIC FOR AI: Template compliance.compliance_cookie_policy_template is missing 'Last Updated:' text.",
        )

        # /terms
        view = self.env.ref("compliance.compliance_terms_of_service_template")
        arch_node = view._get_combined_arch()
        self.assertIsNotNone(arch_node)
        arch_str = etree.tostring(arch_node, encoding="unicode")
        normalized_arch = re.sub(r"\s+", " ", arch_str)
        self.assertIn(
            "Warning: This is the default version",
            normalized_arch,
            "[!] DIAGNOSTIC FOR AI: Template compliance.compliance_terms_of_service_template is missing mandatory default version warning.",
        )
        self.assertIn(
            "It was not produced by a lawyer.",
            normalized_arch,
            "[!] DIAGNOSTIC FOR AI: Template compliance.compliance_terms_of_service_template is missing mandatory legal disclaimer.",
        )
        self.assertIn(
            "Last Updated:",
            normalized_arch,
            "[!] DIAGNOSTIC FOR AI: Template compliance.compliance_terms_of_service_template is missing 'Last Updated:' text.",
        )

        # Tests [@ANCHOR: COMM_compliance_accessibility_statement_template]
        # /accessibility
        view = self.env.ref("compliance.compliance_accessibility_statement_template")
        arch_node = view._get_combined_arch()
        self.assertIsNotNone(arch_node)
        arch_str = etree.tostring(arch_node, encoding="unicode")
        normalized_arch = re.sub(r"\s+", " ", arch_str)
        self.assertIn(
            "Last Updated:",
            normalized_arch,
            "[!] DIAGNOSTIC FOR AI: Template compliance.compliance_accessibility_statement_template is missing 'Last Updated:' text.",
        )
