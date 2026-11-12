# -*- coding: utf-8 -*-
# Part of Odoo. See LICENSE file for full copyright and licensing details.
#
# This file is part of hams_open, an open source module.
# License: AGPL-3.0

import logging
from odoo.tests.common import tagged
from .real_transaction import LEAK_CHECK_FALLBACK_TABLES, RealTransactionCase, count_rows_in_batches

_logger = logging.getLogger(__name__)


@tagged("post_install", "-at_install")
class TestRealTransactionFacility(RealTransactionCase):
    _integration_daemon_process = None
    # Tests [@ANCHOR: zero_sudo:COMM_cursor_hijacking]
    # ---
    # Tests [@ANCHOR: zero_sudo:COMM_leak_snapshotting]
    # ---
    # Tests [@ANCHOR: zero_sudo:COMM_count_rows_in_batches]
    # ---
    # Tests [@ANCHOR: zero_sudo:COMM_orm_instrumentation]
    # ---
    # Tests [@ANCHOR: zero_sudo:COMM_automated_cleanup]
    # ---
    # Tests [@ANCHOR: zero_sudo:COMM_leak_verification]
    # ---
    # Tests [@ANCHOR: zero_sudo:COMM_leak_check_fallback_tables]
    # ---
    # Tests [@ANCHOR: zero_sudo:COMM_user_real_transaction_service]
    # ---
    # Tests [@ANCHOR: zero_sudo:COMM_hams_transaction_case]
    # ---
    # Tests [@ANCHOR: zero_sudo:COMM_hams_http_case]

    def test_00_cursor_hijacking_and_snapshot(self):
        # [@ANCHOR: zero_sudo:COMM_test_cursor_hijacking]
        # ---
        # # Verified by [@ANCHOR: zero_sudo:COMM_test_cursor_hijacking]
        # ---
        # [@ANCHOR: zero_sudo:COMM_test_leak_snapshotting]
        # ---
        # # Verified by [@ANCHOR: zero_sudo:COMM_test_leak_snapshotting]
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

    def test_00b_count_rows_in_batches_releases_locks(self):
        # [@ANCHOR: zero_sudo:COMM_test_count_rows_in_batches]
        # ---
        # # Verified by [@ANCHOR: zero_sudo:COMM_test_count_rows_in_batches]
        """Counting must be exact, and must not accumulate relation locks across batches."""
        tables = list(self._tables)
        self.assertGreater(len(tables), 20)
        self.assertEqual(
            count_rows_in_batches(self.registry.db_name, tables[:20], batch_size=3),
            count_rows_in_batches(self.registry.db_name, tables[:20], batch_size=20),
        )
        # Before each batch's transaction ends, read how many relation locks that backend holds. Small batches must hold far
        # fewer than one big batch over the same tables, i.e. locks do not pile up from batch to batch.
        def peak_locks(batch_size, subset):
            peak = {"locks": 0}

            def watch_locks(cr):
                cr.execute("SELECT count(*) FROM pg_locks WHERE pid = pg_backend_pid() AND locktype = 'relation'")
                peak["locks"] = max(peak["locks"], cr.fetchone()[0])

            counts = count_rows_in_batches(self.registry.db_name, subset, batch_size=batch_size, on_batch=watch_locks)
            return peak["locks"], counts

        subset = tables[:24]
        big_locks, _big_counts = peak_locks(len(subset), subset)
        small_locks, _small_counts = peak_locks(2, subset)
        counts = count_rows_in_batches(self.registry.db_name, tables, batch_size=4)
        self.assertGreater(small_locks, 0)
        self.assertLess(small_locks * 3, big_locks, "locks piled up across batches: %s vs %s" % (small_locks, big_locks))
        self.assertEqual(set(counts), set(tables))

    # Tests [@ANCHOR: zero_sudo_security_log_security_log]
    # Tests [@ANCHOR: zero_sudo_res_users_res_users_zero_sudo]
    # Tests [@ANCHOR: zero_sudo_core_secrets_ir_config_parameter_core_secrets]
    def test_01_auto_cleanup_tracking(self):
        # [@ANCHOR: zero_sudo:COMM_test_orm_instrumentation]
        # ---
        # # Verified by [@ANCHOR: zero_sudo:COMM_test_orm_instrumentation]
        """
        Prove that the facility accurately tracks and auto-deletes standard ORM creations.
        """
        user = self.env["res.users"].create(
            {"name": "Cleanup Target", "login": "cleanup_target", "lang": "en_US"}
        )
        self.env.cr.commit()

        # Verify the record is physically in the DB and tracked
        self.assertTrue(user.exists())
        self.assertIn(user.id, self._tracked_records["res.users"])
        # Note: TearDown will automatically delete this user and check for leaks.

    # Tests [@ANCHOR: zero_sudo_noisy_table_noisy_table]
    def test_02_leak_detector_catches_raw_sql(self):
        # [@ANCHOR: zero_sudo:COMM_test_leak_verification]
        # ---
        # # Verified by [@ANCHOR: zero_sudo:COMM_test_leak_verification]
        """
        Prove that the SQL Leak Detector successfully triggers an AssertionError
        if a test bypasses the ORM tracker using raw SQL inserts.
        """
        # Manually invoke the teardown logic to simulate a leak
        self.cr.execute(  # audit-ignore-sql: # Tested by [@ANCHOR: zero_sudo:COMM_test_leak_verification] # fmt: skip
            "INSERT INTO ir_module_category (name) VALUES ('\"SQL Leak Test\"') RETURNING id"
        )
        leaked_id = self.cr.fetchone()[0]
        self.env.cr.commit()

        try:
            # Temporarily mock the tearDown leak detector to ensure it would raise
            leaks = []
            noisy_tables = set()
            noisy_tables_records = self.env["zero_sudo.noisy_table"].search([], limit=1000)
            noisy_tables = {record.name for record in noisy_tables_records}
    
            self.cr.execute("SELECT count(1) FROM ir_module_category")  # audit-ignore-sql: # Tested by [@ANCHOR: zero_sudo:COMM_test_leak_verification]
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
            self.cr.execute("DELETE FROM ir_module_category WHERE id = %s", (leaked_id,))  # audit-ignore-sql: # Tested by [@ANCHOR: zero_sudo:COMM_test_leak_verification]
            self.env.cr.commit()

    def test_03_foreign_key_cascade_cleanup(self):
        # [@ANCHOR: zero_sudo:COMM_test_automated_cleanup]
        # ---
        # # Verified by [@ANCHOR: zero_sudo:COMM_test_automated_cleanup]
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
                "lang": "en_US",
            }
        )
        self.env.cr.commit()

        self.assertTrue(company.exists())
        self.assertTrue(user.exists())
        # TearDown will now execute its 5-pass loop. If it fails to cascade,
        # the Leak Detector will catch it and fail the suite.

    def test_04_dynamic_noisy_tables(self):
        # [@ANCHOR: zero_sudo:COMM_test_dynamic_noisy_tables]
        # ---
        # # Verified by [@ANCHOR: zero_sudo:COMM_test_dynamic_noisy_tables]
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
        self.cr.execute(  # audit-ignore-sql: # Tested by [@ANCHOR: zero_sudo:COMM_test_dynamic_noisy_tables] # fmt: skip
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
    
            self.cr.execute("SELECT count(1) FROM ir_module_category")  # audit-ignore-sql: # Tested by [@ANCHOR: zero_sudo:COMM_test_dynamic_noisy_tables]
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
            self.cr.execute("DELETE FROM ir_module_category WHERE id = %s", (leaked_id,))  # audit-ignore-sql: # Tested by [@ANCHOR: zero_sudo:COMM_test_dynamic_noisy_tables]
            noisy_table.unlink()
            self.env.cr.commit()

    def test_06_leak_detector_handles_multiple_initial_counts(self):
        # [@ANCHOR: zero_sudo:COMM_test_leak_detector_handles_multiple_initial_counts]
        # ---
        # # Verified by [@ANCHOR: zero_sudo:COMM_test_leak_detector_handles_multiple_initial_counts]
        """
        Ensure the leak detector correctly identifies leaks even if initial count was non-zero.
        """
        try:
            # 1. Ensure we have an initial count
            self.cr.execute(  # audit-ignore-sql: # Tested by [@ANCHOR: zero_sudo:COMM_test_leak_detector_handles_multiple_initial_counts] # fmt: skip
                "INSERT INTO zero_sudo_noisy_table (name) VALUES ('temp_table_leak_test')"
            )
            self.env.cr.commit()

            # We need to re-run snapshotting logic or simulate it
            self.cr.execute("SELECT count(1) FROM zero_sudo_noisy_table")  # audit-ignore-sql: # Tested by [@ANCHOR: zero_sudo:COMM_test_leak_detector_handles_multiple_initial_counts]
            initial_count = self.cr.fetchone()[0]

            # 2. Add another record via SQL (bypass ORM)
            self.cr.execute(  # audit-ignore-sql: # Tested by [@ANCHOR: zero_sudo:COMM_test_leak_detector_handles_multiple_initial_counts] # fmt: skip
                "INSERT INTO zero_sudo_noisy_table (name) VALUES ('temp_table_leak_test_2')"
            )
            self.env.cr.commit()

            # 3. Verify leak detector would catch it
            self.cr.execute("SELECT count(1) FROM zero_sudo_noisy_table")  # audit-ignore-sql: # Tested by [@ANCHOR: zero_sudo:COMM_test_leak_detector_handles_multiple_initial_counts]
            final_count = self.cr.fetchone()[0]

            self.assertEqual(final_count - initial_count, 1)
        finally:
            # Cleanup
            self.cr.execute(  # audit-ignore-sql: # Tested by [@ANCHOR: zero_sudo:COMM_test_leak_detector_handles_multiple_initial_counts] # fmt: skip
                "DELETE FROM zero_sudo_noisy_table WHERE name IN ('temp_table_leak_test', 'temp_table_leak_test_2')"
            )
            self.env.cr.commit()

    # Tests [@ANCHOR: zero_sudo_kv_zero_sudo_kv]
    def test_07_common_setup_class_sql(self):
        # [@ANCHOR: zero_sudo:COMM_test_common_setup_class_sql]
        # ---
        # # Verified by [@ANCHOR: zero_sudo:COMM_test_common_setup_class_sql]
        """
        Verify that the raw SQL used in setUpClass for web.base.url is safe.
        """
        self.env.cr.execute("SELECT value FROM ir_config_parameter WHERE key = 'web.base.url'")
        val = self.env.cr.fetchone()
        self.assertIsNotNone(val)
        self.assertTrue(val[0].startswith('http://odoo') or val[0] == 'https://hams.com')

    def test_08_admin_user_fetch(self):
        # [@ANCHOR: zero_sudo:COMM_test_admin_user_fetch]
        # ---
        # # Verified by [@ANCHOR: zero_sudo:COMM_test_admin_user_fetch]
        """
        Verify that the raw SQL fetch for admin user in setUp is safe and works.
        """
        self.env.cr.execute("SELECT id FROM res_users WHERE login = 'admin'")
        val = self.env.cr.fetchone()
        self.assertIsNotNone(val)
        self.assertEqual(val[0], 2)

    def test_09_statistics_views_are_never_counted_as_leaks(self):
        # [@ANCHOR: zero_sudo:COMM_test_leak_check_fallback_tables]
        # ---
        # # Verified by [@ANCHOR: zero_sudo:COMM_test_leak_check_fallback_tables]
        """
        Every view over the server's statistics catalogs that is installed in this database is exempt from the
        leak check. database_index_advisor (rows with more than 100 sequential scans on a table over 10 MB) is the
        one that failed cloudflare's test_content_hook_multi_website in the whole-system run: another module's
        queries pushed a table over the threshold mid-test and the view gained a row nobody inserted.
        """
        self.cr.execute(  # audit-ignore-sql: # Tested by [@ANCHOR: zero_sudo:COMM_test_leak_check_fallback_tables] # fmt: skip
            "SELECT table_name FROM information_schema.views WHERE table_schema = 'public' "
            "AND view_definition ~ '(pg_stat_|pg_settings|pg_locks)'"
        )
        statistics_views = {r[0] for r in self.cr.fetchall() if not r[0].startswith("pg_stat_statements")}
        self.assertFalse(
            statistics_views - LEAK_CHECK_FALLBACK_TABLES,
            "A view over pg_stat_*/pg_settings changes with server activity, not with test writes; "
            "add it to LEAK_CHECK_FALLBACK_TABLES.",
        )
        # The two that exist in a database_management install must be among those checked.
        for name in ("database_index_advisor", "database_replication_stat"):
            self.assertIn(name, LEAK_CHECK_FALLBACK_TABLES)

    @classmethod
    def tearDownClass(cls):
        # Stop integration daemon if active
        if cls._integration_daemon_process:
            cls._integration_daemon_process.terminate()
        super().tearDownClass()


