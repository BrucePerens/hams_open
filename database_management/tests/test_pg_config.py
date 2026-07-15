# This software is distributed under the terms of the Affero General Public License (AGPL-3).

# -*- coding: utf-8 -*-
from odoo.tests.common import tagged
from odoo.addons.zero_sudo.tests.common import HamsTransactionCase
from odoo.exceptions import UserError
from psycopg2 import sql
from unittest.mock import MagicMock
from contextlib import contextmanager
from odoo.sql_db import Cursor


@tagged("post_install", "-at_install")
class TestPgConfig(HamsTransactionCase):

    def setUp(self):
        super().setUp()
        self.admin = self.env.ref("base.user_admin")

    def test_01_optimization_wizard(self):
        mock_get_uid = self.safe_patch(
            "odoo.addons.zero_sudo.models.security_utils.ZeroSudoSecurityUtils._get_service_uid"
        )
        mock_get_uid.return_value = self.admin.id
        # Tests [@ANCHOR: COMM_pg_optimize_wizard]
        wizard = (
            self.env["pg.optimize.wizard"]
            .with_user(self.admin)
            .create(
                {
                    "ram_gb": 16,
                    "cpu_cores": 8,
                    "storage_type": "ssd",
                    "max_connections": 500,
                }
            )
        )
        mock_cr = MagicMock(spec=Cursor)
        mock_cr.transaction = MagicMock()
        
        @contextmanager
        def mock_cursor():
            yield mock_cr

        self.safe_patch_object(self.env.registry, "cursor", side_effect=mock_cursor)
        res = wizard.action_apply_optimizations()
            
        msg_client = "[!] DIAGNOSTIC FOR AI: pg.optimize.wizard.action_apply_optimizations should return a client action."
        self.assertEqual(res.get("type"), "ir.actions.client", msg_client)

        mock_execute = mock_cr.execute

        # Verify specific calculations
        # 16GB * 0.25 = 4GB = 4096MB
        # 16GB * 0.75 = 12GB = 12288MB
        # min(1024, 16GB * 0.05) = min(1024, 819) = 819MB
        # max(4, (16GB * 0.25) / 500) = max(4, 4096 / 500) = max(4, 8.19) = 8MB

        calls = [
            call[0][0]
            for call in mock_execute.call_args_list
            if isinstance(call[0][0], (sql.SQL, sql.Composed))
        ]
        query_strings = [c.as_string(self.env.cr._obj) for c in calls]

        self.assertTrue(
            any("SET \"shared_buffers\" = '4096MB'" in s for s in query_strings)
        )
        self.assertTrue(
            any("SET \"effective_cache_size\" = '12288MB'" in s for s in query_strings)
        )
        self.assertTrue(
            any("SET \"maintenance_work_mem\" = '819MB'" in s for s in query_strings)
        )
        self.assertTrue(any("SET \"work_mem\" = '8MB'" in s for s in query_strings))
        self.assertTrue(
            any("SET \"max_connections\" = '500'" in s for s in query_strings)
        )
        self.assertTrue(
            any("SET \"random_page_cost\" = '1.1'" in s for s in query_strings)
        )

    def test_02_ha_wizard(self):
        self.safe_patch(
            "odoo.addons.database_management.models.pg_config.PgHaWizard._get_executable",
            return_value="/bin/mock",
        )
        # Tests [@ANCHOR: COMM_pg_ha_wizard]
        wizard = (
            self.env["pg.ha.wizard"]
            .with_user(self.admin)
            .create(
                {
                    "primary_ip": "192.168.1.10",
                    "secondary_ip": "192.168.1.11",
                    "replication_pass": "testpass",
                }
            )
        )
        wizard.action_generate()

        msg_gen = "[!] DIAGNOSTIC FOR AI: pg.ha.wizard state should be 'generated' after generation."
        self.assertEqual(wizard.state, "generated", msg_gen)
        # Assert Patroni Primary details
        self.assertIn("192.168.1.10:8008", wizard.patroni_primary)
        self.assertIn("password: testpass", wizard.patroni_primary)
        self.assertIn("name: node1", wizard.patroni_primary)

        # Assert Patroni Secondary details
        self.assertIn("192.168.1.11:8008", wizard.patroni_secondary)
        self.assertIn("password: testpass", wizard.patroni_secondary)
        self.assertIn("name: node2", wizard.patroni_secondary)

        # Assert PgBouncer details
        self.assertIn("pool_mode = transaction", wizard.pgbouncer_ini)
        self.assertIn("listen_port = 6432", wizard.pgbouncer_ini)

    def test_01b_optimization_wizard_errors(self):
        wizard = (
            self.env["pg.optimize.wizard"]
            .with_user(self.admin)
            .create({"ram_gb": 0, "cpu_cores": 8, "max_connections": 200})
        )
        with self.assertRaises(UserError):
            wizard.action_apply_optimizations()

        wizard2 = (
            self.env["pg.optimize.wizard"]
            .with_user(self.admin)
            .create({"ram_gb": 16, "cpu_cores": 8, "max_connections": 0})
        )
        with self.assertRaises(UserError):
            wizard2.action_apply_optimizations()

    def test_02d_ha_wizard_validation_errors(self):
        # Test invalid IP
        wizard = (
            self.env["pg.ha.wizard"]
            .with_user(self.admin)
            .create({"primary_ip": "invalid-ip", "secondary_ip": "10.0.0.2"})
        )
        msg_ip = "Invalid Primary Node IP format"
        with self.assertRaisesRegex(UserError, msg_ip):
            wizard.action_generate()

        # Test short password
        wizard2 = (
            self.env["pg.ha.wizard"]
            .with_user(self.admin)
            .create(
                {
                    "primary_ip": "10.0.0.1",
                    "secondary_ip": "10.0.0.2",
                    "replication_pass": "short",
                }
            )
        )
        msg_pass = "Password must be at least 8 characters"
        with self.assertRaisesRegex(UserError, msg_pass):
            wizard2.action_generate()

    def test_02b_ha_wizard_missing_binaries(self):
        mock_which = self.safe_patch("shutil.which")
        wizard = (
            self.env["pg.ha.wizard"]
            .with_user(self.admin)
            .create({"primary_ip": "10.0.0.1", "secondary_ip": "10.0.0.2"})
        )

        # Test missing patroni throws error
        def mock_which_side_effect(cmd):
            if cmd == "etcd":
                return "/bin/etcd"
            return None

        mock_which.side_effect = mock_which_side_effect

        with self.assertRaises(UserError):
            wizard.action_generate()

    def test_02c_etcd_auto_download(self):
        mock_ensure = self.safe_patch(
            "odoo.addons.binary_downloader.models.binary_manifest.BinaryManifest.ensure_executable"
        )
        # Prove the system defers to the generalized downloader
        mock_ensure.return_value = "/bin/etcd"
        wizard = (
            self.env["pg.ha.wizard"]
            .with_user(self.admin)
            .create({"primary_ip": "10.0.0.1", "secondary_ip": "10.0.0.2"})
        )
        exe_path = wizard._get_executable("etcd")

        mock_ensure.assert_called_once_with("etcd")
        self.assertEqual(exe_path, "/bin/etcd")

    def test_03_views(self):
        # Tests [@ANCHOR: COMM_test_pg_config_views]
        
        # Tests [@ANCHOR: COMM_db_settings_audit]
        v1 = self.env["database.pg.setting"].get_view(view_type="list")
        self.assertIn("setting", v1["arch"])

        v2 = self.env["pg.optimize.wizard"].get_view(view_type="form")
        self.assertIn("ram_gb", v2["arch"])

        v3 = self.env["pg.ha.wizard"].get_view(view_type="form")
        self.assertIn("primary_ip", v3["arch"])
