# This software is distributed under the terms of the Affero General Public License (AGPL-3).

# -*- coding: utf-8 -*-
from odoo.tests.common import tagged
from odoo.addons.zero_sudo.tests.common import HamsTransactionCase
from unittest.mock import MagicMock, PropertyMock
from odoo.exceptions import UserError
import subprocess
import odoo
@tagged("post_install", "-at_install")
class TestDatabaseManagement(HamsTransactionCase):
    def test_01_vacuum_analyze(self):
        # Tests [@ANCHOR: COMM_vacuum_analyze]
        mock_run = self.safe_patch("subprocess.run")
        self.safe_patch("shutil.which", return_value="/bin/mock")
        mock_res = MagicMock()
        mock_res.returncode = 0
        mock_run.return_value = mock_res

        stat = self.env["database.table.stat"].search(
            [("table_name", "=", "res_users")], limit=1
        )
        msg_table = "[!] DIAGNOSTIC FOR AI: res_users table must exist in database.table.stat view."
        self.assertTrue(
            stat,
            msg_table,
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
        msg_missing = "[!] DIAGNOSTIC FOR AI: action_vacuum_analyze should raise UserError if binary is missing."
        with self.assertRaises(
            UserError,
            msg=msg_missing,
        ):
            stat.action_vacuum_analyze()

        # 2. Non-Zero Exit Code
        mock_which.return_value = "/bin/mock"
        mock_res = MagicMock()
        mock_res.returncode = 1
        mock_res.stderr = "Permission denied"
        mock_run.return_value = mock_res
        msg_fail = "[!] DIAGNOSTIC FOR AI: action_vacuum_analyze should raise UserError if vacuumdb fails."
        with self.assertRaises(UserError, msg=msg_fail):
            stat.action_vacuum_analyze()

        # 3. Timeout
        mock_run.side_effect = subprocess.TimeoutExpired(cmd="vacuumdb", timeout=600)
        msg_timeout = "[!] DIAGNOSTIC FOR AI: action_vacuum_analyze should raise UserError on timeout."
        with self.assertRaises(UserError, msg=msg_timeout):
            stat.action_vacuum_analyze()

    def test_02_bloat_cron(self):
        # Tests [@ANCHOR: COMM_test_dba_cron]
        mock_env = MagicMock()
        with self.safe_patch("odoo.addons.zero_sudo.models.security_utils.ZeroSudoSecurityUtils._get_service_env", return_value=mock_env):
            self.env.ref("database_management.cron_check_bloat")._trigger()
            
    def test_02b_bloat_alert_synergy(self):
        # Tests [@ANCHOR: COMM_bloat_alert_synergy]
        mock_env = MagicMock()
        mock_stat = MagicMock()
        mock_stat.table_name = "test_table"
        mock_stat.bloat_ratio = 50.0
        
        with self.safe_patch_object(type(self.env["database.table.stat"]), "search", return_value=[mock_stat]):
            with self.safe_patch("odoo.addons.zero_sudo.models.security_utils.ZeroSudoSecurityUtils._get_service_env", return_value=mock_env):
                self.env.ref("database_management.cron_check_bloat")._trigger()
        mock_env["pager.incident"].report_incident.assert_called()

    def test_03_db_index_stats(self):
        # Tests [@ANCHOR: COMM_db_index_stats]
        mock_search = self.safe_patch_object(
            type(self.env["database.table.stat"]), "search"
        )
        mock_search.return_value = self.env["database.table.stat"].browse()
        self.env["database.table.stat"].cron_check_bloat()
        mock_search.assert_called_once()

    def test_03_terminate_backend(self):
        # Tests [@ANCHOR: COMM_db_terminate_backend]
        self.env.cr.execute("SELECT pg_terminate_backend(999999)")
        self.assertFalse(self.env.cr.fetchone()[0])
        
        act = self.env["database.activity"].search([], limit=1)
        if act:
            with self.safe_patch("odoo.addons.zero_sudo.models.security_utils.ZeroSudoSecurityUtils._get_service_env") as mock_get_env:
                mock_svc_env = MagicMock()
                mock_get_env.return_value = mock_svc_env
                act.action_terminate_backend()
                mock_svc_env.cr.execute.assert_called()
        self.assertIn("database.activity", self.env)

    def test_04_views(self):
        # Tests [@ANCHOR: COMM_test_dba_view]

        v1 = self.env["database.table.stat"].get_view(view_type="list")
        self.assertIn("table_name", v1["arch"])

        # Tests [@ANCHOR: COMM_db_slow_queries]
        v2 = self.env["database.query.stat"].get_view(view_type="list")
        self.assertIn("query", v2["arch"])

        # Tests [@ANCHOR: COMM_db_active_sessions]
        v3 = self.env["database.activity"].get_view(view_type="list")
        self.assertIn("pid", v3["arch"])

        # Tests [@ANCHOR: COMM_db_index_stats]
        v4 = self.env["database.index.stat"].get_view(view_type="list")
        self.assertIn("index_name", v4["arch"])

        # Tests [@ANCHOR: COMM_db_replication_stats]
        v5 = self.env["database.replication.stat"].get_view(view_type="list")
        self.assertIn("usename", v5["arch"])

        # Tests [@ANCHOR: COMM_db_index_advisor]
        v6 = self.env["database.index.advisor"].get_view(view_type="list")
        self.assertIn("table_name", v6["arch"])

        v7 = self.env["pg.explain.wizard"].get_view(view_type="form")
        self.assertIn("query", v7["arch"])

    def test_06_query_stats_ops(self):
        # Tests [@ANCHOR: COMM_db_slow_queries]
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
        # Tests [@ANCHOR: COMM_db_explain_query]
        stat = self.env["database.query.stat"].search([], limit=1)
        if not stat:
            stat = self.env["database.query.stat"].browse(1)
        msg_only_select = "Only SELECT queries can be analyzed via Explain."
        
        # Bypass ORM cache to set the query field value
        stat._cache.update(stat._record_to_cache({"query": "DELETE FROM res_users"}))
        
        with self.assertRaises(UserError, msg=msg_only_select):
            stat.action_explain_query()

    def test_08_doc_injection(self):
        # Tests [@ANCHOR: COMM_db_doc_injection]
        file_path = odoo.tools.file_open("database_management/data/documentation.html")
        self.assertTrue(file_path, "Documentation file must exist")
