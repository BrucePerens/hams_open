# -*- coding: utf-8 -*-
import collections
import logging
import odoo
from odoo.tests.common import HttpCase, get_db_name
from odoo.modules.registry import Registry
from psycopg2 import sql
from odoo.tools import mute_logger, _

_logger = logging.getLogger(__name__)

# Store the original create method globally to avoid descriptor binding issues
_original_create = odoo.models.BaseModel.create


class RealTransactionCase(HttpCase):
    """
    A testing facility that bypasses Odoo's test cursor wrapping (TransactionCase).
    It provides a real, committable PostgreSQL cursor allowing tests to behave
    exactly like a live production environment.
    """

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
        # [@ANCHOR: cursor_hijacking]
        def _real_cursor_factory(readonly=False):
            return odoo.sql_db.db_connect(self.registry.db_name).cursor()

        if hasattr(self.registry.cursor, "side_effect"):
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
        self.env = odoo.api.Environment(self.cr, odoo.SUPERUSER_ID, {})

        # 2. Snapshot exact table counts
        # [@ANCHOR: leak_snapshotting]
        self.cr.execute(
            "SELECT table_name FROM information_schema.tables WHERE table_schema = 'public'"
        )
        self._tables = [r[0] for r in self.cr.fetchall()]
        self._initial_counts = {}
        for t in self._tables:
            # Securely construct table identifiers using psycopg2.sql
            query = sql.SQL("SELECT count(1) FROM {}").format(sql.Identifier(t))
            self.cr.execute(query)
            self._initial_counts[t] = self.cr.fetchone()[0]

        self._tracked_records = collections.defaultdict(set)

        # 3. Instrument ORM Creation
        # [@ANCHOR: orm_instrumentation]
        # Verified by [@ANCHOR: test_orm_instrumentation]
        def tracking_create(model_self, *args, **kwargs):
            records = _original_create(model_self, *args, **kwargs)
            if records:
                self._tracked_records[model_self._name].update(records.ids)
            return records

        odoo.models.BaseModel.create = tracking_create
        self.addCleanup(setattr, odoo.models.BaseModel, "create", _original_create)

    def tearDown(self):
        # Commit any lingering test state to drop REPEATABLE READ snapshot locks
        # preventing "concurrent update" deadlocks with background HTTP workers.
        try:
            self.env.cr.commit()
        except Exception as e:
            _logger.warning(_("An error occurred during final commit in tearDown: %s"), e)
            self.env.cr.rollback()

        # 2. Automated ORM Cleanup (Multiple passes for Foreign Key cascades)
        # [@ANCHOR: automated_cleanup]
        # Verified by [@ANCHOR: test_automated_cleanup]
        for attempt in range(3):
            pending_deletes = False
            for model_name, ids in list(self._tracked_records.items()):
                if model_name in self.env and ids:
                    try:
                        with self.env.cr.savepoint(), mute_logger(
                            "odoo.sql_db"
                        ), mute_logger("odoo.models.unlink"):
                            records = (
                                self.env[model_name]
                                .with_context(active_test=False)
                                .browse(list(ids))
                                .exists()
                            )
                            if records:
                                records.unlink()
                        # If we reach here, the unlink was successful
                        self._tracked_records[model_name] = set()
                    except Exception as e:
                        pending_deletes = True
                        if attempt == 2:
                            _logger.debug(
                                "Auto-cleanup deferred for %s %s: %s",
                                model_name,
                                ids,
                                e,
                            )
            if not pending_deletes:
                break

        # Commit the automated cleanup to disk
        self.env.cr.commit()

        # 3. Verify No Leaks (Ignoring noisy system logging/chatter tables)
        # [@ANCHOR: leak_verification]
        # Verified by [@ANCHOR: test_leak_verification]
        leaks = []
        noisy_tables = set()
        if "test_real_transaction.noisy_table" in self.env:
            noisy_records = self.env["test_real_transaction.noisy_table"].search(
                [], limit=1000
            )
            noisy_tables = {r.name for r in noisy_records}

        if not noisy_tables:
            noisy_tables = {
                "bus_bus",
                "ir_logging",
                "base_registry_signaling",
                "ir_cron",
                "mail_message",
                "mail_notification",
                "mail_followers",
                "mail_tracking_value",
                "res_groups_users_rel",
                "res_company_users_rel",
                "res_users_log",
                "http_session",
                "database_pg_setting",
                "database_table_stat",
                "database_query_stat",
                "database_activity",
                "database_index_stat",
            }

        for t in self._tables:
            if t in noisy_tables:
                continue
            query = sql.SQL("SELECT count(1) FROM {}").format(sql.Identifier(t))
            self.cr.execute(query)
            final_count = self.cr.fetchone()[0]
            initial_count = self._initial_counts.get(t, 0)
            diff = final_count - initial_count
            if diff != 0:
                leaks.append(f"{t} ({diff:+d})")

        # 4. Close OUR real DB connection
        self.cr.rollback()
        self.cr.close()

        # 5. Hand off teardown to Odoo framework
        # (addCleanup handles cursor restoration)

        self.cr = self._test_cursor
        self.env = self._test_env
        super().tearDown()

        if leaks:
            raise AssertionError(
                _("Database pollution detected! Auto-cleanup failed or raw SQL was used. Leaked records: %s") % ", ".join(leaks)
            )
