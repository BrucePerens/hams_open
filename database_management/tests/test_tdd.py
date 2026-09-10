# This software is distributed under the terms of the Affero General Public License (AGPL-3).

# -*- coding: utf-8 -*-
from odoo.tests.common import tagged
from odoo.addons.zero_sudo.tests.common import HamsTransactionCase

from odoo.exceptions import UserError
from unittest.mock import MagicMock, PropertyMock


@tagged("post_install", "-at_install")
class TestDatabaseManagementTDD(HamsTransactionCase):
    
    def test_tdd_db_stats_transaction(self):
        # Tests [@ANCHOR: COMM_db_explain_query]
        stat = self.env["database.query.stat"].search([], limit=1)
        if not stat:
            stat = self.env["database.query.stat"].browse(1)
        self.safe_patch_object(type(stat), "query", new_callable=PropertyMock, return_value="SELECT 1")
        
        mock_cr = MagicMock()
        mock_cr.fetchone.return_value = ["mock plan"]
        mock_env = MagicMock(cr=mock_cr)
        
        with self.safe_patch(
            "odoo.addons.zero_sudo.models.security_utils.ZeroSudoSecurityUtils._get_service_env",
            return_value=mock_env,
        ):
            stat.action_explain_query()
            
        called = any(
            "SELECT dba_explain_query" in (call[0][0] if isinstance(call[0][0], str) else "")
            for call in mock_cr.execute.call_args_list
        )
        msg = "Must use SELECT dba_explain_query(%s) to prevent injection and avoid latency issue."
        self.assertTrue(called, msg)


    def test_tdd_db_stats_semicolon(self):
        # Tests [@ANCHOR: COMM_db_explain_query]
        stat = self.env["database.query.stat"].search([], limit=1)
        if not stat:
            stat = self.env["database.query.stat"].browse(1)
        
        # Semicolon should no longer raise UserError "Multiple statements are not allowed."
        # If we rely on Postgres logic, we just pass it to explain.
        self.safe_patch_object(type(stat), "query", new_callable=PropertyMock, return_value="SELECT 1;")
        
        mock_cr = MagicMock()
        mock_env = MagicMock(cr=mock_cr)
        
        with self.safe_patch(
            "odoo.addons.zero_sudo.models.security_utils.ZeroSudoSecurityUtils._get_service_env",
            return_value=mock_env,
        ):
            try:
                stat.action_explain_query()
            except UserError as e:
                msg = "Multiple statements are not allowed."
                self.assertNotIn(msg, str(e))

    def test_tdd_pg_config_yaml_injection(self):
        # Tests [@ANCHOR: COMM_pg_ha_wizard]
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

    def test_tdd_pg_config_yaml_injection_via_password(self):
        # Bug-hunt fix (2026-09-09): `replication_pass` is interpolated
        # unescaped into the generated Patroni YAML the same way `cluster_name`
        # is (see test_tdd_pg_config_yaml_injection above), but was never
        # validated for characters that break YAML's plain-scalar syntax --
        # only its length was checked. A password containing a newline could
        # inject an arbitrary sibling key into the same mapping.
        admin = self.env.ref("base.user_admin")
        wizard = self.env["pg.ha.wizard"].with_user(admin).create({
            "primary_ip": "10.0.0.1",
            "secondary_ip": "10.0.0.2",
            "cluster_name": "hams_cluster",
            "superuser_user": "postgres",
            "replication_user": "replicator",
            "replication_pass": "goodpass\n    malicious_key: value",
        })
        self.safe_patch("odoo.addons.database_management.models.pg_config.PgHaWizard._get_executable", return_value="/bin/mock")

        msg = "Replication Password contains characters"
        with self.assertRaisesRegex(UserError, msg):
            wizard.action_generate()

    def test_tdd_pg_config_yaml_injection_via_password_special_chars(self):
        # Same fix as above, exercising the non-control-character hazards
        # PyYAML actually mis-parses in this exact template shape (verified
        # empirically, not assumed from the YAML spec): a leading '#' turns
        # the whole value into a comment (parses as no password at all), and
        # ': ' mid-value reopens a mapping context.
        admin = self.env.ref("base.user_admin")
        for bad_pass in ["#hashfirst123", "goodpass: withcolon", "trailingspace123 "]:
            wizard = self.env["pg.ha.wizard"].with_user(admin).create({
                "primary_ip": "10.0.0.1",
                "secondary_ip": "10.0.0.2",
                "cluster_name": "hams_cluster",
                "superuser_user": "postgres",
                "replication_user": "replicator",
                "replication_pass": bad_pass,
            })
            self.safe_patch("odoo.addons.database_management.models.pg_config.PgHaWizard._get_executable", return_value="/bin/mock")
            msg = "Replication Password contains characters"
            with self.assertRaisesRegex(UserError, msg):
                wizard.action_generate()

    def test_tdd_pg_config_yaml_injection_via_etcd_hosts(self):
        # Bug-hunt fix (2026-09-09): `etcd_hosts` had no validation at all
        # before this fix, despite being interpolated unescaped into the same
        # generated YAML (`etcd: {etcd_config}`) -- the identical injection
        # class as cluster_name/replication_pass above.
        admin = self.env.ref("base.user_admin")
        wizard = self.env["pg.ha.wizard"].with_user(admin).create({
            "primary_ip": "10.0.0.1",
            "secondary_ip": "10.0.0.2",
            "cluster_name": "hams_cluster",
            "superuser_user": "postgres",
            "replication_user": "replicator",
            "replication_pass": "SecureRepPass123!",
            "etcd_hosts": "etcd:2379\n  malicious_key: value",
        })
        self.safe_patch("odoo.addons.database_management.models.pg_config.PgHaWizard._get_executable", return_value="/bin/mock")

        msg = "Invalid Etcd Hosts format"
        with self.assertRaisesRegex(UserError, msg):
            wizard.action_generate()

    def test_tdd_pg_config_etcd_hosts_valid_multi_host(self):
        # The legitimate documented format (comma-separated host:port pairs)
        # must still be accepted after tightening etcd_hosts validation.
        admin = self.env.ref("base.user_admin")
        wizard = self.env["pg.ha.wizard"].with_user(admin).create({
            "primary_ip": "10.0.0.1",
            "secondary_ip": "10.0.0.2",
            "cluster_name": "hams_cluster",
            "superuser_user": "postgres",
            "replication_user": "replicator",
            "replication_pass": "SecureRepPass123!",
            "etcd_hosts": "10.0.0.1:2379,10.0.0.2:2379",
        })
        self.safe_patch("odoo.addons.database_management.models.pg_config.PgHaWizard._get_executable", return_value="/bin/mock")

        wizard.action_generate()
        self.assertEqual(wizard.state, "generated")
        self.assertIn("hosts: [10.0.0.1:2379,10.0.0.2:2379]", wizard.patroni_primary)
