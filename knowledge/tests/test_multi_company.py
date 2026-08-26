# This software is distributed under the terms of the Affero General Public License (AGPL-3).

# -*- coding: utf-8 -*-
from odoo.tests.common import tagged
from odoo.addons.zero_sudo.tests.common import HamsTransactionCase
from odoo.exceptions import AccessError


@tagged("post_install", "-at_install")
class TestKnowledgeArticleMultiCompany(HamsTransactionCase):
    """
    knowledge.article carries 5 company-scoped ir.rule records (admin,
    internal_read, internal_read_published, internal_write, public_read),
    none of which had ever been proven to actually isolate two companies
    from each other. Confirm they do, for both an ordinary internal user
    (internal_read_rule) and a public/guest user (public_read_rule).
    """

    def setUp(self):
        super().setUp()
        self.company_a = self.env["res.company"].create({"name": "Knowledge Co A"})
        self.company_b = self.env["res.company"].create({"name": "Knowledge Co B"})

        # knowledge_article_internal_read_rule (security/*.xml) is scoped to
        # BOTH base.group_portal and base.group_user, and
        # access_knowledge_article_user grants base.group_portal the same
        # model-level read access_knowledge_article_internal grants
        # base.group_user -- so base.group_portal exercises the exact same
        # internal_read_rule this test means to prove, without needing
        # base.group_user, which this codebase's own DOMAIN SANDBOX rule
        # reserves for odoo_facility_service_internal only.
        self.user_a = self.env["res.users"].create(
            {
                "name": "Knowledge User A",
                "login": "knowledge_user_a",
                "company_id": self.company_a.id,
                "company_ids": [(6, 0, [self.company_a.id])],
                "group_ids": [(6, 0, [self.env.ref("base.group_portal").id])],
            }
        )

        # internal_permission defaults to "read" -- any internal user
        # would be able to read this if company scoping didn't apply.
        self.article_a = (
            self.env["knowledge.article"]
            .with_company(self.company_a.id)
            .create({"name": "Article A", "company_id": self.company_a.id})
        )
        self.article_b = (
            self.env["knowledge.article"]
            .with_company(self.company_b.id)
            .create(
                {
                    "name": "Article B",
                    "company_id": self.company_b.id,
                    "is_published": True,
                }
            )
        )

    def test_internal_user_multi_company_isolation(self):
        seen_by_a = (
            self.env["knowledge.article"].with_user(self.user_a).search([])
        )
        self.assertIn(self.article_a, seen_by_a)
        self.assertNotIn(
            self.article_b,
            seen_by_a,
            "[!] DIAGNOSTIC FOR AI: An internal user in Company A MUST NOT "
            "see Company B's knowledge article, even though "
            "internal_permission alone would otherwise allow it.",
        )
        with self.assertRaises(AccessError):
            self.article_b.with_user(self.user_a).read(["name"])

    def test_public_user_multi_company_isolation(self):
        public_user = self.env.ref("base.public_user")
        # The default public user's own company is the main company, not
        # company_b -- a published article belonging to a DIFFERENT
        # company must not be visible to it.
        self.assertNotIn(public_user.company_id, (self.company_a, self.company_b))

        seen_by_public = (
            self.env["knowledge.article"].with_user(public_user).search([])
        )
        self.assertNotIn(
            self.article_b,
            seen_by_public,
            "[!] DIAGNOSTIC FOR AI: A published article belonging to "
            "another company MUST NOT be visible to a public user scoped "
            "to a different company.",
        )
