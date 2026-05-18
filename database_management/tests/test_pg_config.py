# -*- coding: utf-8 -*-
import shutil
import os
from odoo.tests.common import tagged
from odoo.addons.hams_test.tests.real_transaction import RealTransactionCase
from unittest.mock import patch
from odoo.exceptions import UserError

if not hasattr(shutil, "_orig_which"):
    shutil._orig_which = shutil.which
    shutil.which = lambda cmd, mode=os.F_OK, path=None: (
        None if cmd in ("kopia", "etcd") else shutil._orig_which(cmd, mode, path)
    )


@tagged("post_install", "-at_install")
class TestPgConfig(RealTransactionCase):

    def setUp(self):
        super().setUp()
        self.admin = self.env.ref("base.user_admin")

    @patch(
        "odoo.addons.zero_sudo.models.security_utils.ZeroSudoSecurityUtils._get_service_uid"
    )
    def test_01_optimization_wizard(self, mock_get_uid):
        mock_get_uid.return_value = self.admin.id
        # Tests [@ANCHOR: pg_optimize_wizard]
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
        # Execute the optimization (mocked to prevent ActiveSqlTransaction and actual config changes)
        with patch.object(type(self.env.cr), "execute") as mock_execute:
            res = wizard.action_apply_optimizations()
            self.assertEqual(res.get("type"), "ir.actions.client")

            # Verify specific calculations
            # 16GB * 0.25 = 4GB = 4096MB
            # 16GB * 0.75 = 12GB = 12288MB
            # min(1024, 16GB * 0.05) = min(1024, 819) = 819MB
            # max(4, (16GB * 0.25) / 500) = max(4, 4096 / 500) = max(4, 8.19) = 8MB

            calls = [
                call[0][0]
                for call in mock_execute.call_args_list
                if hasattr(call[0][0], "as_string")
            ]
            query_strings = [c.as_string(self.env.cr._obj) for c in calls]

            self.assertTrue(
                any("SET \"shared_buffers\" = '4096MB'" in s for s in query_strings)
            )
            self.assertTrue(
                any(
                    "SET \"effective_cache_size\" = '12288MB'" in s
                    for s in query_strings
                )
            )
            self.assertTrue(
                any(
                    "SET \"maintenance_work_mem\" = '819MB'" in s for s in query_strings
                )
            )
            self.assertTrue(any("SET \"work_mem\" = '8MB'" in s for s in query_strings))
            self.assertTrue(
                any("SET \"max_connections\" = '500'" in s for s in query_strings)
            )
            self.assertTrue(
                any("SET \"random_page_cost\" = '1.1'" in s for s in query_strings)
            )

    @patch(
        "odoo.addons.database_management.models.pg_config.PgHaWizard._get_executable",
        return_value="/bin/mock",
    )
    def test_02_ha_wizard(self, mock_exe):
        # Tests [@ANCHOR: pg_ha_wizard]
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

        self.assertEqual(wizard.state, "generated")
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
            .create({"ram_gb": 0, "cpu_cores": 8})
        )
        with self.assertRaises(UserError):
            wizard.action_apply_optimizations()

    def test_02d_ha_wizard_validation_errors(self):
        # Test invalid IP
        wizard = (
            self.env["pg.ha.wizard"]
            .with_user(self.admin)
            .create({"primary_ip": "invalid-ip", "secondary_ip": "10.0.0.2"})
        )
        with self.assertRaisesRegex(UserError, "Invalid Primary Node IP format"):
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
        with self.assertRaisesRegex(
            UserError, "Password must be at least 8 characters"
        ):
            wizard2.action_generate()

    @patch("shutil.which")
    def test_02b_ha_wizard_missing_binaries(self, mock_which):
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

    @patch(
        "odoo.addons.binary_downloader.models.binary_manifest.BinaryManifest.ensure_executable"
    )
    def test_02c_etcd_auto_download(self, mock_ensure):
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
        # [@ANCHOR: test_pg_config_views]
        # Tests [@ANCHOR: db_settings_audit]
        v1 = self.env["database.pg.setting"].get_view(view_type="list")
        self.assertIn("setting", v1["arch"])

        v2 = self.env["pg.optimize.wizard"].get_view(view_type="form")
        self.assertIn("ram_gb", v2["arch"])

        v3 = self.env["pg.ha.wizard"].get_view(view_type="form")
        self.assertIn("primary_ip", v3["arch"])
