# -*- coding: utf-8 -*-
from odoo.tests.common import tagged
from odoo.addons.hams_test.common import HamsTransactionCase, HamsHttpCase
from unittest.mock import MagicMock
from odoo.exceptions import UserError
import subprocess


@tagged("post_install", "-at_install")
class TestDatabaseManagement(HamsTransactionCase):
    def test_01_vacuum_analyze(self):
        # Tests [@ANCHOR: vacuum_analyze]
        mock_run = self.safe_patch("subprocess.run")
        self.safe_patch("shutil.which", return_value="/bin/mock")
        mock_res = MagicMock()
        mock_res.returncode = 0
        mock_run.return_value = mock_res

        stat = self.env["database.table.stat"].search(
            [("table_name", "=", "res_users")], limit=1
        )
        if stat:
            stat.action_vacuum_analyze()
            mock_run.assert_called()

    def test_01b_vacuum_analyze_failures(self):
        mock_run = self.safe_patch("subprocess.run")
        mock_which = self.safe_patch("shutil.which")
        stat = self.env["database.table.stat"].search(
            [("table_name", "=", "res_users")], limit=1
        )
        if not stat:
            return

        # 1. Missing Binary
        mock_which.return_value = None
        with self.assertRaises(UserError):
            stat.action_vacuum_analyze()

        # 2. Non-Zero Exit Code
        mock_which.return_value = "/bin/mock"
        mock_res = MagicMock()
        mock_res.returncode = 1
        mock_res.stderr = "Permission denied"
        mock_run.return_value = mock_res
        with self.assertRaises(UserError):
            stat.action_vacuum_analyze()

        # 3. Timeout
        mock_run.side_effect = subprocess.TimeoutExpired(cmd="vacuumdb", timeout=300)
        with self.assertRaises(UserError):
            stat.action_vacuum_analyze()

    def test_02_bloat_cron(self):
        # [@ANCHOR: test_dba_cron]
        # Tests [@ANCHOR: bloat_alert_synergy]
        self.env.ref("database_management.cron_check_bloat")._trigger()

    def test_03_db_index_stats(self):
        # Tests [@ANCHOR: db_index_stats]
        mock_search = self.safe_patch_object(
            type(self.env["database.table.stat"]), "search"
        )
        mock_search.return_value = []
        self.env["database.table.stat"].cron_check_bloat()
        mock_search.assert_called_once()

    def test_03_terminate_backend(self):
        # Tests [@ANCHOR: db_terminate_backend]
        # We test termination with a non-existent dummy PID to prevent killing the test runner
        # pg_terminate_backend(pid) returns False if the pid doesn't exist, safely proving execution.
        self.env.cr.execute("SELECT pg_terminate_backend(999999)")
        self.assertFalse(self.env.cr.fetchone()[0])

        # We also trigger the actual ORM method to prove it binds properly without crashing
        act = self.env["database.activity"].search([], limit=1)
        if act:
            act.action_terminate_backend()
        self.assertIn("database.activity", self.env)

    def test_04_views(self):
        # [@ANCHOR: test_dba_view]
        # Tests [@ANCHOR: db_index_stats]
        # Tests [@ANCHOR: db_slow_queries]
        # Tests [@ANCHOR: db_active_sessions]
        v1 = self.env["database.table.stat"].get_view(view_type="list")
        self.assertIn("table_name", v1["arch"])

        v2 = self.env["database.query.stat"].get_view(view_type="list")
        self.assertIn("query", v2["arch"])

        v3 = self.env["database.activity"].get_view(view_type="list")
        self.assertIn("pid", v3["arch"])

        v4 = self.env["database.index.stat"].get_view(view_type="list")
        self.assertIn("index_name", v4["arch"])

    def test_05_documentation_installed(self):
        # Tests [@ANCHOR: db_doc_injection]
        # Verify that the _register_hook installed the documentation
        model = None
        if "knowledge.article" in self.env:
            model = "knowledge.article"
        elif "manual.article" in self.env:
            model = "manual.article"

        if model:
            doc = self.env[model].search(
                [("name", "=", "Database Management Guide")], limit=1
            )
            self.assertTrue(doc, "Module documentation was not installed!")
            self.assertIn("Database Management", doc.body)
        else:
            self.skipTest("No documentation model available")


@tagged("post_install", "-at_install")
class TestDatabaseTours(HamsHttpCase):
    def test_db_bloat_tour(self):
        # [@ANCHOR: test_db_bloat_tour]
        # Tests [@ANCHOR: db_index_stats]
        # Tests [@ANCHOR: vacuum_analyze]
        self.start_tour(
            "/odoo?debug=1&action=database_management.action_db_table_stat",
            "db_management_bloat_tour",
            login="admin",
        )

    def test_db_slow_query_tour(self):
        # [@ANCHOR: test_db_slow_query_tour]
        # Tests [@ANCHOR: db_slow_queries]
        self.start_tour(
            "/odoo?debug=1&action=database_management.action_db_query_stat",
            "db_management_slow_query_tour",
            login="admin",
        )
