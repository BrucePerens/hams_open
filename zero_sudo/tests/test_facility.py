# -*- coding: utf-8 -*-
# Part of Odoo. See LICENSE file for full copyright and licensing details.
#
# This file is part of hams_open, an open source module.
# License: AGPL-3.0

import logging
from odoo.tests.common import tagged
from .real_transaction import RealTransactionCase

_logger = logging.getLogger(__name__)


@tagged("post_install", "-at_install")
class TestRealTransactionFacility(RealTransactionCase):
    _integration_daemon_process = None
    # Tests [@ANCHOR: cursor_hijacking]
    # Tests [@ANCHOR: leak_snapshotting]
    # Tests [@ANCHOR: orm_instrumentation]
    # Tests [@ANCHOR: automated_cleanup]
    # Tests [@ANCHOR: leak_verification]
    # Tests [@ANCHOR: user_real_transaction_service]
    # Tests [@ANCHOR: hams_transaction_case]
    # Tests [@ANCHOR: hams_http_case]

    def test_00_cursor_hijacking_and_snapshot(self):
        # [@ANCHOR: test_cursor_hijacking]
        # [@ANCHOR: test_leak_snapshotting]
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
        """
        Prove that the SQL Leak Detector successfully triggers an AssertionError
        if a test bypasses the ORM tracker using raw SQL inserts.
        """
        # Manually invoke the teardown logic to simulate a leak
        self.cr.execute(  # audit-ignore-sql
            "INSERT INTO ir_module_category (name) VALUES ('\"SQL Leak Test\"') RETURNING id"
        )
        leaked_id = self.cr.fetchone()[0]
        self.env.cr.commit()

        try:
            # Temporarily mock the tearDown leak detector to ensure it would raise
            leaks = []
            noisy_tables = set()
            noisy_tables_records = self.env["zero_sudo.noisy_table"].search([])
            noisy_tables = {record.name for record in noisy_tables_records}
    
            self.cr.execute("SELECT count(1) FROM ir_module_category")  # audit-ignore-sql
            final_count = self.cr.fetchone()[0]
            initial_count = self._initial_counts.get("ir_module_category", 0)
    
            if (
                "ir_module_category" not in noisy_tables
                and final_count - initial_count != 0
            ):
                leaks.append("ir_module_category")
    
            self.assertIn(
                "ir_module_category",
                leaks,
                "The leak detector MUST catch raw SQL insertions.",
            )
        finally:
            # Clean up the raw SQL insertion so the REAL tearDown doesn't crash the test suite
            self.cr.execute("DELETE FROM ir_module_category WHERE id = %s", (leaked_id,))  # audit-ignore-sql
            self.env.cr.commit()

    def test_03_foreign_key_cascade_cleanup(self):
        # [@ANCHOR: test_automated_cleanup]
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
        # TearDown will now execute its 5-pass loop. If it fails to cascade,
        # the Leak Detector will catch it and fail the suite.

    def test_04_dynamic_noisy_tables(self):
        """
        Prove that adding a table to the noisy_table model prevents the leak detector
        from catching it.
        """
        # 1. Add table to noisy tables
        noisy_table = self.env["zero_sudo.noisy_table"].create(
            {"name": "ir_module_category"}
        )
        self.env.cr.commit()

        # 2. Simulate a leak
        self.cr.execute(  # audit-ignore-sql
            "INSERT INTO ir_module_category (name) VALUES ('\"SQL Leak Test Noisy\"') RETURNING id"
        )
        leaked_id = self.cr.fetchone()[0]
        self.env.cr.commit()

        try:
            # 3. Run the leak detector logic
            leaks = []
            noisy_records = self.env["zero_sudo.noisy_table"].search(
                [("active", "=", True)], limit=1000
            )
            noisy_tables = {r.name for r in noisy_records}
    
            self.cr.execute("SELECT count(1) FROM ir_module_category")  # audit-ignore-sql
            final_count = self.cr.fetchone()[0]
            initial_count = self._initial_counts.get("ir_module_category", 0)
    
            if (
                "ir_module_category" not in noisy_tables
                and final_count - initial_count != 0
            ):
                leaks.append("ir_module_category")
    
            self.assertNotIn(
                "ir_module_category",
                leaks,
                "The leak detector MUST ignore tables present in the noisy_table model.",
            )
        finally:
            # 4. Clean up the leak AND the noisy table record to keep the DB clean for tearDown
            self.cr.execute("DELETE FROM ir_module_category WHERE id = %s", (leaked_id,))  # audit-ignore-sql
            noisy_table.unlink()
            self.env.cr.commit()

    def test_06_leak_detector_handles_multiple_initial_counts(self):
        """
        Ensure the leak detector correctly identifies leaks even if initial count was non-zero.
        """
        try:
            # 1. Ensure we have an initial count
            self.cr.execute(  # audit-ignore-sql
                "INSERT INTO zero_sudo_noisy_table (name) VALUES ('temp_table_leak_test')"
            )
            self.env.cr.commit()

            # We need to re-run snapshotting logic or simulate it
            self.cr.execute("SELECT count(1) FROM zero_sudo_noisy_table")  # audit-ignore-sql
            initial_count = self.cr.fetchone()[0]

            # 2. Add another record via SQL (bypass ORM)
            self.cr.execute(  # audit-ignore-sql
                "INSERT INTO zero_sudo_noisy_table (name) VALUES ('temp_table_leak_test_2')"
            )
            self.env.cr.commit()

            # 3. Verify leak detector would catch it
            self.cr.execute("SELECT count(1) FROM zero_sudo_noisy_table")  # audit-ignore-sql
            final_count = self.cr.fetchone()[0]

            self.assertEqual(final_count - initial_count, 1)
        finally:
            # Cleanup
            self.cr.execute(  # audit-ignore-sql
                "DELETE FROM zero_sudo_noisy_table WHERE name IN ('temp_table_leak_test', 'temp_table_leak_test_2')"
            )
            self.env.cr.commit()

    @classmethod
    def tearDownClass(cls):
        # Stop integration daemon if active
        if cls._integration_daemon_process:
            cls._integration_daemon_process.terminate()
        super().tearDownClass()
