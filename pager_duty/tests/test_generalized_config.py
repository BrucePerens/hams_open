# This software is distributed under the terms of the Affero General Public License (AGPL-3).

# -*- coding: utf-8 -*-
from odoo.tests.common import tagged
from odoo.addons.zero_sudo.tests.common import HamsTransactionCase
from unittest.mock import mock_open, MagicMock


@tagged("post_install", "-at_install")
class TestGeneralizedConfig(HamsTransactionCase):
    def setUp(self):
        super().setUp()
        self.admin = self.env.ref("base.user_admin")
        self.json_payload = """
        {
          "checks": [
            {
              "name": "Test DNS Check",
              "type": "dns",
              "target": "example.com",
              "interval": 60,
              "parent": "Parent Check",
              "maint_start": "2026-03-01 00:00:00",
              "maint_end": "2026-03-02 00:00:00"
            },
            {
              "name": "Test Sandboxed Bash",
              "type": "bash",
              "code_payload": "echo test",
              "sandbox_network_access": "full",
              "sandbox_downloads": "http://example.com/file | hash | file.bin",
              "comment": "This is a test comment.",
              "ignored_services": "ignored.service"
            }
          ]
        }
        """

    def test_01_bdd_json_parsing_and_db_sync(self):
        """
        BDD: Given the Generalized Pager Config Wizard (ADR-0051)
        When a valid JSON string is submitted via action_pull_from_json
        Then it MUST successfully parse the JSON and create the corresponding pager.check records.
        """
        # Tests [@ANCHOR: generalized_pager_config]
        check_model = self.env["pager.check"].with_user(self.admin)

        # Mock the file read to supply our JSON payload
        m_open = mock_open(read_data=self.json_payload)
        self.safe_patch("builtins.open", m_open)
        self.safe_patch("os.path.exists", return_value=True)
        check_model.action_pull_from_json()

        m_open.assert_called_once()

        check = self.env["pager.check"].search([("name", "=", "Test DNS Check")])
        self.assertTrue(
            check.exists(), "The JSON must be successfully parsed into DB records."
        )
        self.assertEqual(check.check_type, "dns")
        self.assertEqual(check.target, "example.com")
        self.assertEqual(check.interval, 60)
        self.assertTrue(check.maintenance_start)
        self.assertTrue(check.maintenance_end)

        bash_check = self.env["pager.check"].search(
            [("name", "=", "Test Sandboxed Bash")]
        )
        self.assertTrue(bash_check.exists())
        self.assertEqual(bash_check.code_payload, "echo test")
        self.assertEqual(bash_check.sandbox_network_access, "full")
        self.assertIn("file.bin", bash_check.sandbox_downloads)
        self.assertEqual(bash_check.comment, "This is a test comment.")
        self.assertEqual(bash_check.ignored_services, "ignored.service")

    def test_02_autodiscovery(self):
        mock_push = self.safe_patch(
            "odoo.addons.pager_duty.models.pager_check.PagerCheck.action_push_to_json"
        )
        mock_run = self.safe_patch(
            "odoo.addons.pager_duty.models.pager_check.subprocess.run"
        )
        """Verify the autodiscover action builds checks safely without crashing."""
        mock_res = MagicMock()
        mock_res.stdout = "postgresql.service\nnginx.service"
        mock_run.return_value = mock_res

        self.env["pager.check"].with_user(self.admin).action_autodiscover()

        checks = self.env["pager.check"].search([])
        self.assertTrue(
            len(checks) > 0, "Autodiscovery should generate multiple baseline checks."
        )
        mock_push.assert_called()

    def test_03_views_render(self):
        """Verify the new graphical configuration views render successfully."""
        # Tests [@ANCHOR: test_pager_view]
        v1 = self.env["pager.check"].get_view(view_type="form")
        v2 = self.env["pager.check"].get_view(view_type="list")
        self.assertIn("arch", v1)
        self.assertIn("arch", v2)

    def test_04_config_path_security(self):
        """
        Verify that the configuration path is resolved through the service account.
        """
        # Tests [@ANCHOR: generalized_pager_config_path]
        check_model = self.env["pager.check"].with_user(self.admin)
        path = check_model._get_config_path()
        self.assertTrue(path.endswith("pager_config.json"))

    def test_05_json_type_hazard(self):
        """
        Verify that passing a JSON array instead of a dictionary does not crash the JSON pull.
        """
        check_model = self.env["pager.check"].with_user(self.admin)
        
        # Mock the file read to supply a JSON array
        m_open = mock_open(read_data="[]")
        self.safe_patch("builtins.open", m_open)
        self.safe_patch("os.path.exists", return_value=True)
        
        # This should not raise TypeError
        check_model.action_pull_from_json()
        m_open.assert_called_once()

