# -*- coding: utf-8 -*-
# Part of Odoo. See LICENSE file for full copyright and licensing details.
#
# This file is part of hams_open, an open source module.
# License: AGPL-3.0

import collections
import logging
import odoo
from odoo.tests.common import HttpCase, get_db_name
from odoo.modules.registry import Registry
import psycopg2
from psycopg2 import sql
from odoo.tools import mute_logger, _
from odoo.addons.zero_sudo.tests.common import SafePatchMixin, wait_for_werkzeug_threads
import unittest.mock

_logger = logging.getLogger(__name__)

# Store the original create method globally to avoid descriptor binding issues
_original_create = odoo.models.BaseModel.create


class RealTransactionCase(HttpCase, SafePatchMixin):
    """
    A testing facility that bypasses Odoo's test cursor wrapping (TransactionCase).
    It provides a real, committable PostgreSQL cursor allowing tests to behave
    exactly like a live production environment.
    """

    @classmethod
    def setUpClass(cls):
        super().setUpClass()
        with cls.registry.cursor() as cr:
            cr.execute(  # audit-ignore-sql: Tested by [@ANCHOR: COMM_test_common_setup_class_sql]
                "INSERT INTO ir_config_parameter (key, value) VALUES ('web.base.url', 'https://hams.com') "
                "ON CONFLICT (key) DO UPDATE SET value='https://hams.com'"
            )

    def setUp(self):
        super().setUp()

        # HttpCase creates a TestCursor which acquires Odoo's global test lock.
        # We stash it so super().tearDown() can cleanly dispose of it later.
        self._test_cursor = self.cr
        self._test_env = self.env

        self.registry = Registry(get_db_name())

        # 1. Safely hijack the Mock object injected by Odoo's test framework.
        # By changing the side_effect rather than deleting the attribute, we prevent
        # Werkzeug deadlocks without crashing unittest.mock during tearDown.
        # [@ANCHOR: COMM_cursor_hijacking]
        # Verified by [@ANCHOR: COMM_test_cursor_hijacking]
        def _real_cursor_factory(readonly=False):
            return odoo.sql_db.db_connect(self.registry.db_name).cursor()

        if isinstance(self.registry.cursor, unittest.mock.Mock):
            _original_cursor = self.registry.cursor
            _original_side_effect = self.registry.cursor.side_effect
            self.registry.cursor.side_effect = _real_cursor_factory

            def _restore_cursor():
                _original_cursor.side_effect = _original_side_effect

            self.addCleanup(_restore_cursor)
        else:
            _original_cursor = self.registry.cursor
            self.registry.cursor = _real_cursor_factory
            self.addCleanup(setattr, self.registry, "cursor", _original_cursor)

        # Provision a true PostgreSQL cursor for the test thread
        self.cr = self.registry.cursor()

        # Use the standard Admin user (ID 2) for test setup privileges instead of the banned SUPERUSER_ID cheat
        self.cr.execute("SELECT id FROM res_users WHERE login = 'admin'")  # audit-ignore-sql: Tested by [@ANCHOR: COMM_test_admin_user_fetch]
        row = self.cr.fetchone()
        admin_id = row[0] if row else 2
        self.env = odoo.api.Environment(self.cr, admin_id, {})

        # 2. Snapshot exact table counts
        # [@ANCHOR: COMM_leak_snapshotting]
        # Verified by [@ANCHOR: COMM_test_leak_snapshotting]
        self.cr.execute(  # audit-ignore-sql: Tested by [@ANCHOR: COMM_test_leak_snapshotting]
            "SELECT table_name FROM information_schema.tables WHERE table_schema = 'public' AND table_name NOT LIKE 'pg_stat_statements%'"
        )
        self._tables = [r[0] for r in self.cr.fetchall()]
        self._initial_counts = {}
        for t in self._tables:
            # Securely construct table identifiers using psycopg2.sql
            query = sql.SQL("SELECT count(1) FROM {}").format(sql.Identifier(t))
            self.cr.execute(query)  # audit-ignore-sql: Tested by [@ANCHOR: COMM_test_leak_snapshotting]
            self._initial_counts[t] = self.cr.fetchone()[0]

        self._tracked_records = collections.defaultdict(set)

        # 3. Instrument ORM Creation
        # [@ANCHOR: COMM_orm_instrumentation]
        # Verified by [@ANCHOR: COMM_test_orm_instrumentation]

        def tracking_create(model_self, *args, **kwargs):
            records = _original_create(model_self, *args, **kwargs)
            if records:
                self._tracked_records[model_self._name].update(records.ids)
            return records

        odoo.models.BaseModel.create = tracking_create
        self.addCleanup(setattr, odoo.models.BaseModel, "create", _original_create)
        self.addCleanup(self._real_teardown)

    def _real_teardown(self):
        # Guarantee that our raw PostgreSQL cursor is ALWAYS rolled back and closed
        # even if an ORM AccessError occurs during the leak verification phase.
        # Wait for any lingering backend HTTP threads to finish, preventing teardown serialization failures.
        wait_for_werkzeug_threads(timeout=5.0)

        try:
            # Rollback any lingering, uncommitted test state to drop REPEATABLE READ
            # snapshot locks and abort pending dirty-form submissions.
            try:
                self.env.cr.rollback()
            except Exception as e:  # audit-ignore-catch-all
                _logger.warning("Ignored error during initial teardown rollback: %s", e)

            # 2. Automated ORM Cleanup (Multiple passes for Foreign Key cascades)
            # Verified by [@ANCHOR: COMM_test_automated_cleanup]
            for attempt in range(5):
                pending_deletes = False
                for model_name, ids in reversed(list(self._tracked_records.items())):
                    if ids:
                        model_env = self.env[model_name]
                        try:
                            with self.env.cr.savepoint(), mute_logger(
                                "odoo.sql_db"
                            ), mute_logger("odoo.models.unlink"):
                                records = (
                                    model_env.with_context(active_test=False)
                                    .browse(list(ids))
                                    .exists()
                                )
                                if records:
                                    records.with_user(2).unlink()  # audit-ignore-sql: Administrative test environment cleanup
                            self._tracked_records[model_name] = set()
                        except (
                            psycopg2.IntegrityError,
                            psycopg2.OperationalError,
                            odoo.exceptions.AccessError,
                            odoo.exceptions.UserError,
                            odoo.exceptions.RedirectWarning,
                            odoo.exceptions.ValidationError,
                        ) as e:
                            pending_deletes = True
                            if attempt == 4:
                                _logger.info(
                                    "Auto-cleanup failed for %s %s after 5 attempts: %s",
                                    model_name,
                                    ids,
                                    e,
                                )
                        except Exception as e:  # audit-ignore-catch-all
                            pending_deletes = True
                            _logger.error(
                                "Unexpected error during auto-cleanup of %s %s: %s",
                                model_name,
                                ids,
                                e,
                                exc_info=True,
                            )
                if not pending_deletes:
                    break

            # Commit the automated cleanup to disk
            self.env.cr.commit()

            # 3. Verify No Leaks
            # Verified by [@ANCHOR: COMM_test_leak_verification]
            leaks = []
            noisy_tables = set()
            try:
                noisy_records = self.env["zero_sudo.noisy_table"].search(
                    [("active", "=", True)], limit=1000
                )
                noisy_tables = {r.name for r in noisy_records}
            except Exception as e:  # audit-ignore-catch-all
                _logger.warning("Could not fetch noisy tables during teardown: %s", e)

            fallback_tables = {
                "bus_bus",
                "ir_logging",
                "base_registry_signaling",
                "ir_cron",
                "mail_message",
                "mail_notification",
                "mail_followers",
                "mail_tracking_value",
                "mail_mail",
                "res_groups_users_rel",
                "res_company_users_rel",
                "res_users_log",
                "http_session",
                "database_pg_setting",
                "database_table_stat",
                "database_query_stat",
                "database_activity",
                "database_index_stat",
                "ir_attachment",
                "ir_model_data",
                "website_visitor",
                "website_track",
                "ir_ui_view",
                "cloudflare_purge_queue",
                "res_groups_implied_rel",
                "res_users_apikeys",
                "ir_cron_progress",
                "orm_signaling_stable",
                "ir_config_parameter",
                "gamification_challenge_users_rel",
                "gamification_goal",
                "ir_cron_trigger",
            }
            noisy_tables.update(fallback_tables)

            for t in self._tables:
                if t in noisy_tables:
                    continue
                query = sql.SQL("SELECT count(1) FROM {}").format(sql.Identifier(t))
                self.cr.execute(query)  # audit-ignore-sql: Tested by [@ANCHOR: COMM_test_automated_cleanup]
                final_count = self.cr.fetchone()[0]
                initial_count = self._initial_counts.get(t, 0)
                diff = final_count - initial_count
                if diff != 0:
                    leaks.append(f"{t} ({diff:+d})")

            if leaks:
                raise AssertionError(
                    _(
                        "Database pollution detected! Auto-cleanup failed or raw SQL was used. Leaked records: %s"
                    )
                    % ", ".join(leaks)
                )

        finally:
            # 4. Close OUR real DB connection NO MATTER WHAT
            try:
                self.cr.rollback()
                self.registry.clear_cache()
                self.env.clear()
                self.cr.close()
            except Exception as e:  # audit-ignore-catch-all
                _logger.error(
                    "Failed to cleanly close DB connection during teardown: %s", e
                )

            # 4. Cleanly restore the underlying HttpCase TestCursor so its own teardown succeeds.
            # Verified by [@ANCHOR: COMM_test_cursor_restoration]
            self.registry.cursor = self._test_cursor
            self.cr = self._test_cursor
            self.env = self._test_env
