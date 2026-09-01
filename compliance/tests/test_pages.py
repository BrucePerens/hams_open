# -*- coding: utf-8 -*-
# Copyright © Bruce Perens K6BP.
# SPDX-License-Identifier: AGPL-3.0-or-later
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
            msg = f"[!] DIAGNOSTIC FOR AI: Page for {url} should exist in \'website.page\'. Check compliance/data/legal_pages_data.xml for missing records."
            self.assertIn(url, found_urls, msg)

        # Found live 2026-08-29 via a usability audit persona: legal_pages_data.xml
        # created these website.page records without a "name" field. Odoo's own
        # generic site search (website/controllers/main.py's autocomplete, when
        # search_type='all') sorts combined results with `r.get('name', '')`,
        # which crashes with a TypeError ('<' not supported between 'bool' and
        # 'str') the moment any record in the mix has name=False rather than a
        # missing key -- exactly what an unset Char field on a real record looks
        # like. A page missing here doesn't just render blank, it can crash
        # site search for every visitor.
        for page in pages:
            msg = f"[!] DIAGNOSTIC FOR AI: website.page for {page.url} has no name set. This crashes Odoo's generic site search (TypeError sorting bool vs str) the moment it's a search result. Set a name in compliance/data/legal_pages_data.xml."
            self.assertTrue(page.name, msg)

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
                    msg = f"[!] DIAGNOSTIC FOR AI: Boilerplate page for {page.url} should be unpublished because a custom one exists in the same scope. Check compliance/hooks.py logic."
                    self.assertFalse(page.is_published, msg)
                else:
                    msg = f"[!] DIAGNOSTIC FOR AI: Boilerplate page for {page.url} should be published since no custom one exists in the same scope. Check compliance/hooks.py logic."
                    self.assertTrue(page.is_published, msg)
            else:
                msg = f"[!] DIAGNOSTIC FOR AI: Custom page for {page.url} should be published."
                self.assertTrue(page.is_published, msg)


@tagged("post_install", "-at_install")
class TestCompliancePagesHttp(HamsHttpCase):

    def test_pages_reachable(self):
        """Verify that legal pages are reachable via HTTP."""
        # Tests [@ANCHOR: COMM_compliance_privacy_policy_template]

        # Tests [@ANCHOR: COMM_compliance_cookie_policy_template]

        # Tests [@ANCHOR: COMM_compliance_terms_of_service_template]

        # Tests [@ANCHOR: COMM_story_automatic_legal_pages]
        response = self.url_open("/privacy")
        msg_status = f"[!] DIAGNOSTIC FOR AI: Page /privacy should be reachable (200 OK). Got {response.status_code}. Ensure the website.page record is published."
        self.assertEqual(response.status_code, 200, msg_status)
        msg_text = "[!] DIAGNOSTIC FOR AI: Page /privacy should contain boilerplate content. Check the rendering."
        self.assertTrue(bool(re.search(r"Policy|Terms", response.text)), msg_text)

        response = self.url_open("/cookie-policy")
        msg_status = f"[!] DIAGNOSTIC FOR AI: Page /cookie-policy should be reachable (200 OK). Got {response.status_code}. Ensure the website.page record is published."
        self.assertEqual(response.status_code, 200, msg_status)
        msg_text = "[!] DIAGNOSTIC FOR AI: Page /cookie-policy should contain boilerplate content. Check the rendering."
        self.assertTrue(bool(re.search(r"Policy|Terms", response.text)), msg_text)

        response = self.url_open("/terms")
        msg_status = f"[!] DIAGNOSTIC FOR AI: Page /terms should be reachable (200 OK). Got {response.status_code}. Ensure the website.page record is published."
        self.assertEqual(response.status_code, 200, msg_status)
        msg_text = "[!] DIAGNOSTIC FOR AI: Page /terms should contain boilerplate content. Check the rendering."
        self.assertTrue(bool(re.search(r"Policy|Terms", response.text)), msg_text)

        response = self.url_open("/accessibility")
        msg_status = f"[!] DIAGNOSTIC FOR AI: Page /accessibility should be reachable (200 OK). Got {response.status_code}. Ensure the website.page record is published."
        self.assertEqual(response.status_code, 200, msg_status)
        msg_text = "[!] DIAGNOSTIC FOR AI: Page /accessibility should contain boilerplate content. Check the rendering."
        self.assertTrue(bool(re.search(r"Policy|Terms", response.text)), msg_text)

    def test_generic_site_search_does_not_crash_on_legal_pages(self):
        # Found live 2026-08-29: /website/search?search=forum&order=name+asc
        # (Odoo's generic site search, search_type='all') threw an unhandled
        # 500 -- TypeError: '<' not supported between instances of 'bool' and
        # 'str' -- in odoo/addons/website/controllers/main.py's autocomplete,
        # sorting combined results by `r.get('name', '')`. Root cause: the
        # /privacy, /cookie-policy, /terms, and /accessibility website.page
        # records had no `name` set, so they sorted as name=False against
        # every other result's string name. Reproduces the exact request path
        # that crashed, using search terms that should surface these pages.
        for term in ("privacy", "terms", "cookie", "accessibility"):
            response = self.url_open(f"/website/search?search={term}&order=name+asc")
            msg = f"[!] DIAGNOSTIC FOR AI: Generic site search for '{term}' returned {response.status_code}, expected 200. A website.page (or any other searchable record) with an unset name field crashes this route's sort."
            self.assertEqual(response.status_code, 200, msg)

    def test_protects_hams_page_reachable_and_registered(self):
        """
        The new central "How Hams.com Protects Hams" page must resolve
        publicly (no login required) and be registered in the shared
        compliance.document registry so it surfaces via /compliance, the
        same as Privacy/Terms/LoTW Trust/Transmitter Safety/etc. Also
        checks it actually links to the two existing, real trust pages
        rather than re-explaining them (this page is meant to be a central
        index, not a duplicate).
        """
        # Tests [@ANCHOR: compliance:protects_hams_page]
        response = self.url_open("/protects-hams")
        msg_status = f"[!] DIAGNOSTIC FOR AI: Page /protects-hams should be reachable (200 OK). Got {response.status_code}."
        self.assertEqual(response.status_code, 200, msg_status)
        self.assertIn("How Hams.com Protects Hams", response.text)
        self.assertIn('href="/lotw-trust"', response.text)
        self.assertIn('href="/transmitter-safety"', response.text)
        self.assertIn('href="/relay/source"', response.text)

        public_uid = self.env.ref("base.public_user").id
        registered_urls = (
            self.env["compliance.document"]
            .with_user(public_uid)
            .search([])
            .mapped("url")
        )
        self.assertIn("/protects-hams", registered_urls)

    def test_compliance_index_route_lists_only_active_documents(self):
        """Verify the actual /compliance HTTP route, not just its template."""
        # Tests [@ANCHOR: COMM_compliance_index_route]
        svc_uid = self.env["zero_sudo.security.utils"]._get_service_uid(
            "compliance.user_compliance_service"
        )
        Document = self.env["compliance.document"].with_user(svc_uid)
        active_doc = Document.create(
            {"name": "Active Disclosure Doc XYZ", "url": "/active-doc-xyz"}
        )
        inactive_doc = Document.create(
            {
                "name": "Inactive Disclosure Doc XYZ",
                "url": "/inactive-doc-xyz",
                "active": False,
            }
        )
        response = self.url_open("/compliance")
        msg_status = f"[!] DIAGNOSTIC FOR AI: /compliance should be reachable (200 OK). Got {response.status_code}."
        self.assertEqual(response.status_code, 200, msg_status)
        self.assertIn(
            active_doc.name,
            response.text,
            "[!] DIAGNOSTIC FOR AI: /compliance should list active compliance.document records.",
        )
        self.assertNotIn(
            inactive_doc.name,
            response.text,
            "[!] DIAGNOSTIC FOR AI: /compliance should NOT list inactive (active=False) compliance.document records -- the controller's domain filters on active=True.",
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
            "Disclaimer: This document is provided",
            normalized_arch,
            "[!] DIAGNOSTIC FOR AI: Template compliance.compliance_privacy_policy_template is missing mandatory default version warning.",
        )
        self.assertIn(
            "Please consult with legal counsel",
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
            "Disclaimer: This document is provided",
            normalized_arch,
            "[!] DIAGNOSTIC FOR AI: Template compliance.compliance_cookie_policy_template is missing mandatory default version warning.",
        )
        self.assertIn(
            "Please consult with legal counsel",
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
            "Disclaimer: This document is provided",
            normalized_arch,
            "[!] DIAGNOSTIC FOR AI: Template compliance.compliance_terms_of_service_template is missing mandatory default version warning.",
        )
        self.assertIn(
            "Please consult with legal counsel",
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

    def test_compliance_index_view(self):
        """Verify that the compliance index template renders correctly."""
        # [@ANCHOR: COMM_test_compliance_index_view]
        view = self.env.ref("compliance.compliance_index_template")
        # Tests [@ANCHOR: COMM_compliance_index_route]
        arch_node = view._get_combined_arch()
        self.assertIsNotNone(arch_node)
        arch_str = etree.tostring(arch_node, encoding="unicode")
        normalized_arch = re.sub(r"\s+", " ", arch_str)
        msg = "[!] DIAGNOSTIC FOR AI: compliance_index_template missing title."
        self.assertIn("Regulatory Compliance &amp; Policies", normalized_arch, msg)
