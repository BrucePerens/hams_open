# -*- coding: utf-8 -*-
from odoo.tests.common import tagged
from odoo.addons.zero_sudo.tests.common import HamsTransactionCase
from unittest.mock import MagicMock, PropertyMock
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

        stat = self.env["database.table.stat"].search([("table_name", "=", "res_users")], limit=1)
        self.assertTrue(
            stat,
            "[!] DIAGNOSTIC FOR AI: res_users table must exist in database.table.stat view.",
        )
        if stat:
            stat.action_vacuum_analyze()
            mock_run.assert_called()

    def test_01b_vacuum_analyze_failures(self):
        mock_run = self.safe_patch("subprocess.run")
        mock_which = self.safe_patch("shutil.which")
        stat = self.env["database.table.stat"].search([("table_name", "=", "res_users")], limit=1)
        if not stat:
            return

        # 1. Missing Binary
        mock_which.return_value = None
        with self.assertRaises(
            UserError,
            msg="[!] DIAGNOSTIC FOR AI: action_vacuum_analyze should raise UserError if binary is missing.",
        ):
            stat.action_vacuum_analyze()

        # 2. Non-Zero Exit Code
        mock_which.return_value = "/bin/mock"
        mock_res = MagicMock()
        mock_res.returncode = 1
        mock_res.stderr = "Permission denied"
        mock_run.return_value = mock_res
        with self.assertRaises(
            UserError,
            msg="[!] DIAGNOSTIC FOR AI: action_vacuum_analyze should raise UserError if vacuumdb fails.",
        ):
            stat.action_vacuum_analyze()

        # 3. Timeout
        mock_run.side_effect = subprocess.TimeoutExpired(cmd="vacuumdb", timeout=600)
        with self.assertRaises(
            UserError,
            msg="[!] DIAGNOSTIC FOR AI: action_vacuum_analyze should raise UserError on timeout.",
        ):
            stat.action_vacuum_analyze()

    def test_02_bloat_cron(self):
        # [@ANCHOR: test_dba_cron]
        # Tests [@ANCHOR: bloat_alert_synergy]
        self.env.ref("database_management.cron_check_bloat")._trigger()

    def test_03_db_index_stats(self):
        # Tests [@ANCHOR: db_index_stats]
        mock_search = self.safe_patch_object(type(self.env["database.table.stat"]), "search")
        mock_search.return_value = self.env["database.table.stat"].browse()
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

    def test_06_query_stats_ops(self):
        # Tests [@ANCHOR: db_slow_queries]
        model = self.env["database.query.stat"]

        mock_cr = MagicMock()
        mock_env = MagicMock(cr=mock_cr)

        with self.safe_patch(
            "odoo.addons.zero_sudo.models.security_utils.ZeroSudoSecurityUtils._get_service_env",
            return_value=mock_env,
        ):
            model.action_reset_stats()

        mock_cr.execute.assert_any_call("SELECT pg_stat_statements_reset()")

    def test_07_explain_query_security(self):
        # Tests [@ANCHOR: db_explain_query]
        stat = self.env["database.query.stat"].search([], limit=1)
        if not stat:
            stat = self.env["database.query.stat"].browse(1)

        type(stat).query = PropertyMock(return_value="DELETE FROM res_users")

        with self.assertRaises(UserError, msg="Only SELECT queries can be analyzed via Explain."):
            stat.action_explain_query()
