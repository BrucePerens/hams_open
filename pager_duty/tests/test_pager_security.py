# -*- coding: utf-8 -*-
from odoo.tests.common import tagged
from odoo.addons.zero_sudo.tests.common import HamsTransactionCase
from odoo.exceptions import AccessError


@tagged("post_install", "-at_install")
class TestPagerSecurity(HamsTransactionCase):
    def setUp(self):
        super().setUp()
        self.admin = self.env.ref("base.user_admin")
        self.user_std = self.env["res.users"].create(
            {
                "name": "Std",
                "login": "std_pager",
                "group_ids": [(4, self.env.ref("base.group_portal").id)],
            }
        )

        web_grp = self.env.ref(
            "user_websites.group_user_websites_user", raise_if_not_found=False
        )
        groups_web = [self.env.ref("base.group_portal").id] + (
            [web_grp.id] if web_grp else []
        )
        self.user_web = self.env["res.users"].create(
            {"name": "Web", "login": "web_pager", "group_ids": [(6, 0, groups_web)]}
        )

        ham_grp = self.env.ref("base.group_portal", raise_if_not_found=False)
        groups_ham = [self.env.ref("base.group_portal").id] + (
            [ham_grp.id] if ham_grp else []
        )
        self.user_ham = self.env["res.users"].create(
            {"name": "Ham", "login": "ham_pager", "group_ids": [(6, 0, groups_ham)]}
        )

        swl_grp = self.env.ref("base.group_portal", raise_if_not_found=False)
        groups_swl = [self.env.ref("base.group_portal").id] + (
            [swl_grp.id] if swl_grp else []
        )
        self.user_swl = self.env["res.users"].create(
            {"name": "Swl", "login": "swl_pager", "group_ids": [(6, 0, groups_swl)]}
        )

        self.public_user = self.env.ref("base.public_user")

        # Initialize an incident as admin so we have data to test against
        self.incident = (
            self.env["pager.incident"]
            .with_user(self.admin)
            .create(
                {"source": "test", "severity": "low", "description": "Security test"}
            )
        )

    def test_01_multi_persona_isolation(self):
        """
        BDD: Given ADR-0050 Proxy Ownership IDOR (Multi-Persona Mandate)
        When standard personas attempt to interact with the NOC incident table
        Then they MUST be violently rejected by the ORM, as only Pager Admins and Service Accounts have access.
        """
        for user in [
            self.user_std,
            self.user_web,
            self.user_ham,
            self.user_swl,
            self.public_user,
        ]:
            with self.assertRaises(
                AccessError, msg=f"{user.name} MUST NOT be able to read incidents."
            ):
                self.incident.with_user(user).read(["name"])

            with self.assertRaises(
                AccessError, msg=f"{user.name} MUST NOT be able to write incidents."
            ):
                self.incident.with_user(user).write({"status": "acknowledged"})
                self.env.flush_all()

            with self.assertRaises(
                AccessError, msg=f"{user.name} MUST NOT be able to create incidents."
            ):
                self.env["pager.incident"].with_user(user).create(
                    {"source": "x", "severity": "low", "description": "y"}
                )
                self.env.flush_all()

            with self.assertRaises(
                AccessError, msg=f"{user.name} MUST NOT be able to unlink incidents."
            ):
                self.incident.with_user(user).unlink()

    def test_02_documentation_injection(self):
        """
        Verify that documentation is correctly injected during post_init_hook.
        Tests [@ANCHOR: doc_inject_pager_duty]
        """
        # The documentation is created in post_init_hook, so it should exist if tests are running.
        article_model = self.env.get("knowledge.article")
        if not article_model:
            # Skip if knowledge.article is not available (e.g. standard Odoo without manual_library)
            return

        article = article_model.search(
            [("name", "=", "Pager Duty & Generalized Monitoring")], limit=1
        )
        self.assertTrue(article, "Pager Duty documentation article should exist.")
        self.assertIn("Pager Duty", article.body)
        self.assertIn("Generalized Monitoring", article.body)
