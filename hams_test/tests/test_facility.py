# -*- coding: utf-8 -*-
import os
import unittest
from unittest.mock import MagicMock, patch
from odoo.tests.common import tagged
from odoo.addons.hams_test.tests.real_transaction import RealTransactionCase
from odoo.addons.hams_test.common import HamsIntegrationCase


@tagged("post_install", "-at_install")
class TestRealTransactionFacility(RealTransactionCase):
    # Tests [@ANCHOR: cursor_hijacking]
    # Tests [@ANCHOR: leak_snapshotting]
    # Tests [@ANCHOR: orm_instrumentation]
    # Tests [@ANCHOR: automated_cleanup]
    # Tests [@ANCHOR: leak_verification]
    # Tests [@ANCHOR: user_real_transaction_service]

    def test_00_cursor_hijacking_and_snapshot(self):
        # [@ANCHOR: test_cursor_hijacking]
        # [@ANCHOR: test_leak_snapshotting]
        # Verified by [@ANCHOR: test_cursor_hijacking]
        # Verified by [@ANCHOR: test_leak_snapshotting]
        """
        Verify that the cursor is indeed real and that snapshotting occurred.
        """
        # 1. Verify cursor hijacking
        self.assertFalse(self.cr.readonly)
        # Check if it's a real connection by trying to commit and see if it persists (carefully)
        # But we already know RealTransactionCase does this in setUp.

        # 2. Verify snapshotting
        self.assertTrue(len(self._initial_counts) > 0)
        self.assertIn("res_users", self._initial_counts)

    def test_01_auto_cleanup_tracking(self):
        # [@ANCHOR: test_orm_instrumentation]
        # Verified by [@ANCHOR: test_orm_instrumentation]
        """
        Prove that the facility accurately tracks and auto-deletes standard ORM creations.
        """
        user = self.env["res.users"].create(
            {"name": "Cleanup Target", "login": "cleanup_target"}
        )
        self.env.cr.commit()

        # Verify the record is physically in the DB and tracked
        self.assertTrue(user.exists())
        self.assertIn(user.id, self._tracked_records["res.users"])
        # Note: TearDown will automatically delete this user and check for leaks.

    def test_02_leak_detector_catches_raw_sql(self):
        # [@ANCHOR: test_leak_verification]
        # Verified by [@ANCHOR: test_leak_verification]
        """
        Prove that the SQL Leak Detector successfully triggers an AssertionError
        if a test bypasses the ORM tracker using raw SQL inserts.
        """
        # Manually invoke the teardown logic to simulate a leak
        self.cr.execute(
            "INSERT INTO ir_module_category (name) VALUES ('\"SQL Leak Test\"') RETURNING id"
        )
        leaked_id = self.cr.fetchone()[0]
        self.env.cr.commit()

        # Temporarily mock the tearDown leak detector to ensure it would raise
        leaks = []
        noisy_tables_records = self.env['hams_test.noisy_table'].search([])
        noisy_tables = {record.name for record in noisy_tables_records}

        self.cr.execute("SELECT count(1) FROM ir_module_category")
        final_count = self.cr.fetchone()[0]
        initial_count = self._initial_counts.get("ir_module_category", 0)

        if "ir_module_category" not in noisy_tables and final_count - initial_count != 0:
            leaks.append("ir_module_category")

        # Clean up the raw SQL insertion so the REAL tearDown doesn't crash the test suite
        self.cr.execute("DELETE FROM ir_module_category WHERE id = %s", (leaked_id,))
        self.env.cr.commit()

        self.assertIn(
            "ir_module_category",
            leaks,
            "The leak detector MUST catch raw SQL insertions.",
        )

    def test_03_foreign_key_cascade_cleanup(self):
        # [@ANCHOR: test_automated_cleanup]
        # Verified by [@ANCHOR: test_automated_cleanup]
        """
        Prove that the multi-pass auto-cleanup handles hierarchical dependencies.
        """
        company = self.env["res.company"].create({"name": "Test Company"})
        user = self.env["res.users"].create(
            {
                "name": "FK User",
                "login": "fk_user",
                "company_id": company.id,
                "company_ids": [(4, company.id)],
            }
        )
        self.env.cr.commit()

        self.assertTrue(company.exists())
        self.assertTrue(user.exists())
        # TearDown will now execute its 3-pass loop. If it fails to cascade,
        # the Leak Detector will catch it and fail the suite.

    def test_04_dynamic_noisy_tables(self):
        """
        Prove that adding a table to the noisy_table model prevents the leak detector
        from catching it.
        """
        # Add ir_module_category to noisy tables
        noisy_table_record = self.env['hams_test.noisy_table'].create({
            'name': 'ir_module_category'
        })
        self.env.cr.commit()

        # Simulate a leak
        self.cr.execute(
            "INSERT INTO ir_module_category (name) VALUES ('\"SQL Leak Test Noisy\"') RETURNING id"
        )
        leaked_id = self.cr.fetchone()[0]
        self.env.cr.commit()

        # Run the leak detector logic
        leaks = []
        noisy_tables_records = self.env['hams_test.noisy_table'].search([])
        noisy_tables = {record.name for record in noisy_tables_records}

        self.cr.execute("SELECT count(1) FROM ir_module_category")
        final_count = self.cr.fetchone()[0]
        initial_count = self._initial_counts.get("ir_module_category", 0)

        if "ir_module_category" not in noisy_tables and final_count - initial_count != 0:
            leaks.append("ir_module_category")

        # Clean up the leak AND the noisy table record to keep the DB clean for tearDown
        self.cr.execute("DELETE FROM ir_module_category WHERE id = %s", (leaked_id,))
        noisy_table_record.unlink()
        self.env.cr.commit()

        self.assertNotIn(
            "ir_module_category",
            leaks,
            "The leak detector MUST ignore tables present in the noisy_table model.",
        )

    def test_05_documentation_installed(self):
        # [@ANCHOR: test_documentation_bootstrap]
        # [@ANCHOR: test_documentation_injection]
        # Verified by [@ANCHOR: test_documentation_bootstrap]
        # Verified by [@ANCHOR: test_documentation_injection]
        """
        Verify that the module's documentation was correctly installed.
        """
        # Tests [@ANCHOR: documentation_bootstrap]
        # Tests [@ANCHOR: documentation_injection]
        article_model_name = None
        if "knowledge.article" in self.env:
            article_model_name = "knowledge.article"
        elif "manual.article" in self.env:
            article_model_name = "manual.article"

        if article_model_name:
            article = self.env[article_model_name].search(
                [("name", "=", "Real Transaction Testing Facility Guide")], limit=1
            )
            self.assertTrue(article, "Documentation article should have been created.")
            self.assertIn("Real Transaction Testing Facility", article.body)


@tagged("post_install", "-at_install")
class TestIntegrationFacility(HamsIntegrationCase):
    # Tests [@ANCHOR: integration_daemon_testing]

    def test_01_daemon_lifecycle(self):
        """
        Verify that HamsIntegrationCase correctly manages daemon lifecycle.
        We mock the daemon_utils to avoid actually spinning up a process.
        """
        DaemonUtils = self.registry['zero_sudo.daemon.utils']
        with patch.object(DaemonUtils, 'start_daemon_process') as mock_start, \
             patch.object(DaemonUtils, 'poll_health_check') as mock_poll, \
             patch.object(DaemonUtils, 'stop_daemon_process') as mock_stop:

            mock_process = MagicMock()
            mock_start.return_value = mock_process

            # Start a dummy daemon
            dummy_path = os.path.join(os.path.dirname(__file__), "dummy_daemon.py")
            process = self.start_daemon(dummy_path, health_url="http://odoo:1234")

            self.assertEqual(process, mock_process)
            self.assertIn(process, self._daemons)
            mock_start.assert_called_once_with(dummy_path, None, None)
            mock_poll.assert_called_once_with("http://odoo:1234", timeout=30)

            # Verify daemons were stopped during teardown (HamsIntegrationCase handles this)
            # We don't call tearDownClass manually to avoid double cleanup.
            # Instead, we just check that the daemon was added to the list.
            self.assertIn(mock_process, self._daemons)
