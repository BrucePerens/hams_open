# This software is distributed under the terms of the Affero General Public License (AGPL-3).

# -*- coding: utf-8 -*-
from odoo.tests.common import tagged
from odoo.addons.zero_sudo.tests.common import HamsTransactionCase
from psycopg2 import sql as psql
from odoo.exceptions import UserError
from unittest.mock import MagicMock, PropertyMock

@tagged("post_install", "-at_install")
class TestDatabaseManagementTDD(HamsTransactionCase):
    
    def test_tdd_db_stats_transaction(self):
        stat = self.env["database.query.stat"].search([], limit=1)
        if not stat:
            stat = self.env["database.query.stat"].browse(1)
        type(stat).query = PropertyMock(return_value="SELECT 1")
        
        mock_cr = MagicMock()
        mock_env = MagicMock(cr=mock_cr)
        
        with self.safe_patch(
            "odoo.addons.zero_sudo.models.security_utils.ZeroSudoSecurityUtils._get_service_env",
            return_value=mock_env,
        ):
            # This should not raise UserError about semicolon anymore if we just pass "SELECT 1"
            # And it should call "SET LOCAL default_transaction_read_only = on;"
            stat.action_explain_query()
            
        called = any(
            isinstance(call[0][0], psql.Composed) and "default_transaction_read_only" in call[0][0].as_string(self.env.cr._obj)
            for call in mock_cr.execute.call_args_list
        )
        self.assertTrue(called, "Transaction must be set to local read only using psycopg2")

    def test_tdd_db_stats_semicolon(self):
        stat = self.env["database.query.stat"].search([], limit=1)
        if not stat:
            stat = self.env["database.query.stat"].browse(1)
        
        # Semicolon should no longer raise UserError "Multiple statements are not allowed."
        # If we rely on Postgres logic, we just pass it to explain.
        type(stat).query = PropertyMock(return_value="SELECT 1;")
        
        mock_cr = MagicMock()
        mock_env = MagicMock(cr=mock_cr)
        
        with self.safe_patch(
            "odoo.addons.zero_sudo.models.security_utils.ZeroSudoSecurityUtils._get_service_env",
            return_value=mock_env,
        ):
            try:
                stat.action_explain_query()
            except UserError as e:
                self.assertNotIn("Multiple statements are not allowed.", str(e))

    def test_tdd_pg_config_yaml_injection(self):
        admin = self.env.ref("base.user_admin")
        wizard = self.env["pg.ha.wizard"].with_user(admin).create({
            "primary_ip": "10.0.0.1",
            "secondary_ip": "10.0.0.2",
            "cluster_name": "hams_cluster\n  malicious_key: value",
            "superuser_user": "postgres",
            "replication_user": "replicator",
            "replication_pass": "SecureRepPass123!"
        })
        self.safe_patch("odoo.addons.database_management.models.pg_config.PgHaWizard._get_executable", return_value="/bin/mock")
        
        with self.assertRaises(UserError):
            wizard.action_generate()
