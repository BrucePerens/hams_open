# This software is distributed under the terms of the Affero General Public License (AGPL-3).

# -*- coding: utf-8 -*-
from odoo.tests.common import tagged
from odoo.addons.zero_sudo.tests.common import HamsTransactionCase
from odoo.exceptions import AccessError
from odoo.tools import mute_logger


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

    def test_02_rpc_ensure_executable_security(self):
        """
        Tests [@ANCHOR: rpc_ensure_executable_security]
        """
        # Ensure the binary_downloader service account exists for the test
        if not self.env.ref(
            "binary_downloader.user_binary_downloader_service", raise_if_not_found=False
        ):
            service_user = self.env["res.users"].create(
                {
                    "name": "Binary Service",
                    "login": "binary_service",
                    "is_service_account": True,
                }
            )
            self.env["ir.model.data"].create(
                {
                    "name": "user_binary_downloader_service",
                    "module": "binary_downloader",
                    "model": "res.users",
                    "res_id": service_user.id,
                }
            )

        CheckModel = self.env["pager.check"]
        # Should fail for non-allow-listed command
        res = CheckModel.rpc_ensure_executable("rm")
        self.assertEqual(res["status"], "error")
        self.assertIn("Command not in allow-list", res["message"])

        # Should pass (or at least get past allow-list) for allowed command
        with mute_logger('odoo.addons.pager_duty.models.pager_check'):
            res = CheckModel.rpc_ensure_executable("ping")
        self.assertNotEqual(res.get("message"), "Command not in allow-list.")

    def test_03_documentation_injection(self):
        """
        Verify that documentation is correctly injected during post_init_hook.
        Tests [@ANCHOR: doc_inject_pager_duty]
        """
        # The documentation is created by _bootstrap_knowledge_docs, so it should exist if tests are running.
        self.env["ir.module.module"]._bootstrap_knowledge_docs()
        article_model = self.env.get("knowledge.article")
        if not article_model:
            # Skip if knowledge.article is not available (e.g. standard Odoo without knowledge)
            return

        article = article_model.search(
            [("name", "=", "Pager Duty & Generalized Monitoring")], limit=1
        )
        self.assertTrue(article, "Pager Duty documentation article should exist.")
        self.assertIn("Pager Duty", article.body)
        self.assertIn("Generalized Monitoring", article.body)

    def test_04_multi_tenant_isolation(self):
        """
        Verify that multi-tenancy rules isolate records by company_id.
        """
        company_a = self.env.company
        company_b = self.env["res.company"].create({"name": "Company B"})

        # Admin user is in Company A
        self.admin.write({"company_ids": [(6, 0, [company_a.id])], "company_id": company_a.id})

        # Create Admin B in Company B
        admin_b = self.env["res.users"].create(
            {
                "name": "Admin B",
                "login": "admin_b",
                "group_ids": [(6, 0, [self.env.ref("base.group_portal").id, self.env.ref("pager_duty.group_pager_admin").id, self.env.ref("base.group_multi_company").id])],
                "company_ids": [(6, 0, [company_b.id])],
                "company_id": company_b.id,
            }
        )

        # Create records in Company A
        check_a = self.env["pager.check"].with_user(self.admin).create({"name": "Check A", "company_id": company_a.id, "check_type": "heartbeat"})
        inc_a = self.env["pager.incident"].with_user(self.admin).create({"source": "A", "company_id": company_a.id})
        
        # Create records in Company B
        check_b = self.env["pager.check"].with_user(admin_b).create({"name": "Check B", "company_id": company_b.id, "check_type": "heartbeat"})
        inc_b = self.env["pager.incident"].with_user(admin_b).create({"source": "B", "company_id": company_b.id})

        # Admin A should only see Company A's records
        checks_a_sees = self.env["pager.check"].with_user(self.admin).search([])
        self.assertIn(check_a, checks_a_sees)
        self.assertNotIn(check_b, checks_a_sees)

        incs_a_sees = self.env["pager.incident"].with_user(self.admin).search([])
        self.assertIn(inc_a, incs_a_sees)
        self.assertNotIn(inc_b, incs_a_sees)

        # Admin B should only see Company B's records
        checks_b_sees = self.env["pager.check"].with_user(admin_b).search([])
        self.assertIn(check_b, checks_b_sees)
        self.assertNotIn(check_a, checks_b_sees)

        incs_b_sees = self.env["pager.incident"].with_user(admin_b).search([])
        self.assertIn(inc_b, incs_b_sees)
        self.assertNotIn(inc_a, incs_b_sees)

    def test_05_multi_website_isolation(self):
        """
        Verify that multi-tenancy rules isolate records by website_id.
        """
        website_a = self.env["website"].create({"name": "Website A"})
        website_b = self.env["website"].create({"name": "Website B"})

        # Admin user is assigned to Website A
        self.admin.write({"website_id": website_a.id})

        # Create Admin B in Website B
        admin_b = self.env["res.users"].create(
            {
                "name": "Admin B Website B",
                "login": "admin_b_web_b",
                "group_ids": [(6, 0, [self.env.ref("base.group_portal").id, self.env.ref("pager_duty.group_pager_admin").id])],
                "website_id": website_b.id,
            }
        )

        # Create records in Website A
        check_a = self.env["pager.check"].with_user(self.admin).create({"name": "Check Web A", "website_id": website_a.id, "check_type": "heartbeat"})
        inc_a = self.env["pager.incident"].with_user(self.admin).create({"source": "Web A", "website_id": website_a.id})
        
        # Create records in Website B
        check_b = self.env["pager.check"].with_user(admin_b).create({"name": "Check Web B", "website_id": website_b.id, "check_type": "heartbeat"})
        inc_b = self.env["pager.incident"].with_user(admin_b).create({"source": "Web B", "website_id": website_b.id})

        # Admin A should only see Website A's records
        checks_a_sees = self.env["pager.check"].with_user(self.admin).search([])
        self.assertIn(check_a, checks_a_sees)
        self.assertNotIn(check_b, checks_a_sees)

        incs_a_sees = self.env["pager.incident"].with_user(self.admin).search([])
        self.assertIn(inc_a, incs_a_sees)
        self.assertNotIn(inc_b, incs_a_sees)

        # Admin B should only see Website B's records
        checks_b_sees = self.env["pager.check"].with_user(admin_b).search([])
        self.assertIn(check_b, checks_b_sees)
        self.assertNotIn(check_a, checks_b_sees)

        incs_b_sees = self.env["pager.incident"].with_user(admin_b).search([])
        self.assertIn(inc_b, incs_b_sees)
        self.assertNotIn(inc_a, incs_b_sees)
