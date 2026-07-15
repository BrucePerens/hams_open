# This software is distributed under the terms of the Affero General Public License (AGPL-3).

# -*- coding: utf-8 -*-
from odoo.tests.common import tagged
from odoo.addons.zero_sudo.tests.common import HamsTransactionCase
from odoo.exceptions import AccessError


@tagged("post_install", "-at_install")
class TestDbSecurity(HamsTransactionCase):
    def setUp(self):
        super().setUp()
        self.admin = self.env.ref("base.user_admin")
        self.user_std = self.env["res.users"].create(
            {
                "name": "Std",
                "login": "std_db",
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
            {
                "name": "Web",
                "login": "web_db",
                "group_ids": [(6, 0, groups_web)],
            }
        )

        ham_grp = self.env.ref("base.group_portal", raise_if_not_found=False)
        groups_ham = [self.env.ref("base.group_portal").id] + (
            [ham_grp.id] if ham_grp else []
        )
        self.user_ham = self.env["res.users"].create(
            {
                "name": "Ham",
                "login": "ham_db",
                "group_ids": [(6, 0, groups_ham)],
            }
        )

        swl_grp = self.env.ref("base.group_portal", raise_if_not_found=False)
        groups_swl = [self.env.ref("base.group_portal").id] + (
            [swl_grp.id] if swl_grp else []
        )
        self.user_swl = self.env["res.users"].create(
            {
                "name": "Swl",
                "login": "swl_db",
                "group_ids": [(6, 0, groups_swl)],
            }
        )

        self.public_user = self.env.ref("base.public_user")

        # Fetch existing SQL View records for testing read isolation
        self.table_stat = (
            self.env["database.table.stat"].with_user(self.admin).search([], limit=1)
        )
        self.pg_setting = (
            self.env["database.pg.setting"].with_user(self.admin).search([], limit=1)
        )

    def test_01_security(self):
        # Tests [@ANCHOR: COMM_db_security_prefetch]
        self.assertIn("database.table.stat", self.env)
        """
        BDD: Given ADR-0050 Proxy Ownership IDOR (Multi-Persona Mandate)
        When standard personas attempt to interact with the database APM tools
        Then they MUST be violently rejected by the ORM, as only System Admins have access.
        """
        for user in [
            self.user_std,
            self.user_web,
            self.user_ham,
            self.user_swl,
            self.public_user,
        ]:
            # Assert SQL Views are protected
            if self.table_stat:
                msg_table = f"{user.name} MUST NOT be able to read DB table stats."
                with self.assertRaises(AccessError, msg=msg_table):
                    self.table_stat.with_user(user).read(["table_name"])
            if self.pg_setting:
                msg_pg = f"{user.name} MUST NOT be able to read PG configurations."
                with self.assertRaises(AccessError, msg=msg_pg):
                    self.pg_setting.with_user(user).read(["name"])

            # Assert Wizards are protected
            msg_opt = f"{user.name} MUST NOT be able to access the Optimize Wizard."
            with self.assertRaises(AccessError, msg=msg_opt):
                self.env["pg.optimize.wizard"].with_user(user).create({"ram_gb": 16})
                self.env.flush_all()

            msg_ha = f"{user.name} MUST NOT be able to access the HA Wizard."
            with self.assertRaises(AccessError, msg=msg_ha):
                self.env["pg.ha.wizard"].with_user(user).create(
                    {"primary_ip": "10.0.0.1"}
                )
                self.env.flush_all()

    def test_02_view_models_readonly(self):
        """
        Ensure `perm_write`, `perm_create`, and `perm_unlink` are set to `0` for view-backed models.
        """
        view_models = [
            "database.table.stat",
            "database.query.stat",
            "database.activity",
            "database.index.stat",
            "database.pg.setting",
            "database.index.advisor",
            "database.replication.stat",
        ]
        model_ids = self.env["ir.model"].search([("model", "in", view_models)])
        accesses = self.env["ir.model.access"].search([("model_id", "in", model_ids.ids)])
        for access in accesses:
            model = access.model_id.model
            self.assertFalse(access.perm_write, f"{model} should not have perm_write")
            self.assertFalse(access.perm_create, f"{model} should not have perm_create")
            self.assertFalse(access.perm_unlink, f"{model} should not have perm_unlink")

    def test_03_admin_success(self):
        """
        Ensure self.admin can read protected SQL views and wizards.
        """
        if self.table_stat:
            self.assertTrue(self.table_stat.with_user(self.admin).read(["table_name"]))
        if self.pg_setting:
            self.assertTrue(self.pg_setting.with_user(self.admin).read(["name"]))

        opt_wiz = self.env["pg.optimize.wizard"].with_user(self.admin).create({"ram_gb": 16})
        self.assertTrue(opt_wiz)
        
        ha_wiz = self.env["pg.ha.wizard"].with_user(self.admin).create({"primary_ip": "10.0.0.1"})
        self.assertTrue(ha_wiz)
