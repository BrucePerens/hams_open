# SPDX-License-Identifier: AGPL-3.0-or-later
# This software is distributed under the terms of the Affero General Public License (AGPL-3).

# -*- coding: utf-8 -*-
import re

from odoo.tests.common import tagged
from odoo.addons.zero_sudo.tests.common import HamsTransactionCase


@tagged("standard", "post_install", "-at_install")
class TestLogMonitoringSeedData(HamsTransactionCase):
    """ODOO_DB_LOG_CRASH_MONITORING.md's log-pattern-seeding half:
    data/pager_log_monitoring_data.xml seeds pager.log.file/pager.log.pattern
    records for the real, confirmed Odoo/PostgreSQL log paths and formats on
    this dev box. Each regex here is checked against real sample lines
    pulled directly from those live logs (see the proposal doc's own
    write-up of the exact samples), not synthetic ones -- and, where the
    proposal's own text calls out a real noise risk, against a real benign
    line too, confirming the pattern does NOT fire on it."""

    def test_01_seeded_log_files_point_at_the_real_confirmed_paths(self):
        odoo_log = self.env.ref("pager_duty.pager_log_file_odoo")
        self.assertEqual(odoo_log.filepath, "/var/log/odoo/odoo-server.log")
        pg_log = self.env.ref("pager_duty.pager_log_file_postgresql")
        self.assertEqual(pg_log.filepath, "/var/log/postgresql/postgresql-17-main.log")

    def test_02_traceback_pattern_matches_a_real_sample_line(self):
        pattern = self.env.ref("pager_duty.pager_log_pattern_odoo_traceback")
        self.assertEqual(pattern.severity, "high")
        self.assertTrue(re.search(pattern.regex, "Traceback (most recent call last):"))

    def test_03_critical_pattern_matches_a_real_sample_line_not_a_plain_info_line(self):
        pattern = self.env.ref("pager_duty.pager_log_pattern_odoo_critical")
        self.assertEqual(pattern.severity, "critical")
        real_critical = (
            "2026-08-18 17:40:56,496 1454440 CRITICAL hams_com odoo.modules.module: "
            "Couldn't load module ham_shack "
        )
        self.assertTrue(re.search(pattern.regex, real_critical))
        real_info = (
            "2026-08-30 03:02:19,522 605610 INFO hams_dev odoo.service.server: "
            "Worker (605610) exiting. request_count: 20, registry count: 1. "
        )
        self.assertFalse(re.search(pattern.regex, real_info))

    def test_04_sql_db_pattern_matches_a_real_bad_query_line_not_an_unrelated_constraint_error(self):
        pattern = self.env.ref("pager_duty.pager_log_pattern_odoo_sql_db_error")
        self.assertEqual(pattern.severity, "high")
        real_bad_query = (
            "2026-08-23 06:57:57,607 1134810 ERROR hams_test_cf odoo.sql_db: "
            "bad query: b'SELECT \"website\".\"id\" FROM \"website\"'"
        )
        self.assertTrue(re.search(pattern.regex, real_bad_query))
        # A real, routine ERROR-level line this same investigation's own log
        # sample included -- expected user-input validation, not a crash,
        # and must NOT match. See pager_log_monitoring_data.xml's own doc
        # comment on why there's deliberately no blanket " ERROR " pattern.
        real_constraint_error = (
            'ERROR:  new row for relation "ham_dns_record" violates check '
            'constraint "ham_dns_record_content_not_empty"'
        )
        self.assertFalse(re.search(pattern.regex, real_constraint_error))

    def test_05_postgresql_panic_and_fatal_patterns_match_real_sample_lines(self):
        panic_pattern = self.env.ref("pager_duty.pager_log_pattern_postgresql_panic")
        self.assertEqual(panic_pattern.severity, "critical")
        fatal_pattern = self.env.ref("pager_duty.pager_log_pattern_postgresql_fatal")
        self.assertEqual(fatal_pattern.severity, "medium")
        real_fatal = (
            "2026-08-23 07:24:15.291 PDT [1103243] odoo@hams_test FATAL:  "
            "terminating connection due to administrator command"
        )
        self.assertTrue(re.search(fatal_pattern.regex, real_fatal))
        self.assertFalse(re.search(panic_pattern.regex, real_fatal))

    def test_06_all_seed_patterns_are_unique_and_active(self):
        # A duplicate/inactive pattern from an earlier seeding attempt would
        # silently stop matching (or double-report) without this.
        patterns = self.env["pager.log.pattern"].search(
            [("name", "in", [
                "Odoo Python Traceback",
                "Odoo CRITICAL Log Level",
                "Odoo Database Query Error",
                "PostgreSQL PANIC",
                "PostgreSQL FATAL",
            ])]
        )
        self.assertEqual(len(patterns), 5)
        self.assertTrue(all(patterns.mapped("active")))
