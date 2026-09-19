# -*- coding: utf-8 -*-
# Part of Odoo. See LICENSE file for full copyright and licensing details.
#
# This file is part of hams_open, an open source module.
# SPDX-License-Identifier: AGPL-3.0-or-later

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

    # night_shift_todo/high/test-runner-hangs-before-any-tour-starts-9a60a362.md's own root
    # cause: odoo/tests/suite.py's TestSuite.run() sets odoo.modules.module.current_test to a
    # freshly-constructed (but not yet setUp()'d) test instance BEFORE this class's own
    # setUpClass() runs -- and setUpClass() below does a real committed INSERT, real wall-clock
    # time. Any external HTTP request landing during that whole window (an /odoo/health poll from
    # this project's own test harness, confirmed live -- this is the exact class
    # TestSettingsAndCache, a real subclass, was caught mid-setUpClass() in) hits Odoo core's
    # assertCanOpenTestCursor(), which reads self.http_request_allow_all -- normally set only in
    # BaseCase.setUp() (instance-level, once per TEST METHOD), which hasn't run yet for a
    # class-level health-check request. That crashes with AttributeError, which _serve_db
    # silently swallows and serves the request without a real cursor (a 302 instead of the
    # expected health-check response), which Odoo core's own internal health-check retry loop
    # never recognizes as ready -- until this project's OWN test-runner watchdog gives up and
    # kills the whole run after several minutes of apparent silence. These two class-level
    # defaults close the gap: read only during the vulnerable pre-setUp() window, unconditionally
    # overwritten by BaseCase.setUp() the moment a real test method starts, so this changes
    # nothing about normal, non-racing test execution.
    http_request_key = ""
    http_request_allow_all = False

    @classmethod
    def setUpClass(cls):
        # A real, physically-committed cursor, NOT cls.registry.cursor(): under
        # --test-enable, Odoo's own test harness mocks registry.cursor at the
        # CLASS level (before setUp()'s own _real_cursor_factory monkeypatch
        # ever runs) to hand out TestCursor savepoint proxies over one shared,
        # never-really-committed suite transaction. cls.registry.cursor()
        # here used to be exactly that -- its own "commit" was a savepoint
        # RELEASE, not a real COMMIT, so the insert below was invisible to
        # every later real connection (self.cr, opened via db_connect() in
        # setUp()'s _real_cursor_factory) for the rest of the class's tests.
        # Root-caused 2026-09-17 by comparing txid_current() before/after:
        # the "post-commit" reread reported the SAME txid as before the
        # insert -- proof no real transaction boundary had been crossed.
        # db_connect(...).cursor() bypasses the mock entirely, matching how
        # _real_cursor_factory itself gets a real cursor.
        #
        # night_shift_todo/high/test-runner-hangs-before-any-tour-starts-9a60a362.md's own
        # root cause, source-traced against Odoo core: this block USED to run AFTER
        # super().setUpClass(), which resolves to HttpCase.setUpClass()
        # (odoo/tests/common.py:2220-2228). That method sets cls.cr = cls.registry.cursor()
        # (a real, unpatched cursor -- the TestCursor mock is only installed a few lines
        # later by registry_enter_test_mode_cls(), so cls.cr itself is never a savepoint
        # proxy) and then calls ICP.set_param('web.base.url', cls.base_url()), which does a
        # real UPDATE of this exact row through cls.cr and never commits it (a
        # TransactionCase's cls.cr is deliberately rolled back, not committed, at class
        # teardown -- that is the whole point of the isolation it provides). That UPDATE's
        # row lock therefore sat held for the rest of the class's setUpClass() window. The
        # second, separate db_connect() connection below then tried to UPSERT the SAME row
        # and blocked on that lock forever -- a class deadlocking against its own parent's
        # write, one statement after another, in the same call stack (confirmed live via
        # py-spy + pg_stat_activity: the blocked backend's own in-flight query was this
        # exact INSERT, and the row-locking backend was cls.cr's connection, "idle in
        # transaction" on its own uncommitted UPDATE of the same id).
        #
        # The fix is this reordering. This db_connect() cursor is a genuinely SEPARATE,
        # short-lived connection: it opens, writes, commits for real, and closes -- all
        # inside this "with" block, before cls.cr / cls.registry / cls.env exist at all
        # (get_db_name() needs none of them; setUp() below already calls it the same way).
        # By the time super().setUpClass() runs and HttpCase.setUpClass() performs its own
        # real UPDATE of this row through cls.cr, this connection has already committed and
        # gone -- there is no longer a second, concurrent connection for it to collide with.
        # The two writes still happen, in this order, but sequentially in wall-clock time on
        # non-overlapping connections, not concurrently on the same locked row. (Reordering
        # alone was considered and dismissed once already in this same to-do, on the
        # assumption that HttpCase.setUpClass()'s own write would still race a still-open
        # competing connection regardless of order -- that assumption didn't account for
        # this connection being transient and already closed by the time HttpCase's write
        # runs, which is what actually eliminates the collision.)
        #
        # This does NOT make HttpCase's own HTTP/tour machinery see the wrong host: url_open(),
        # browser_js() and xmlrpc_url all build requests from cls.base_url() directly in
        # Python (HOST + cls.http_port()), never by reading the 'web.base.url' ICP value back
        # out of the database -- confirmed by reading HttpCase's own source. So it is safe for
        # the real, cross-connection-committed value of 'web.base.url' to differ from
        # cls.base_url() for the rest of the class's life; nothing in Odoo core's own request
        # mechanics depends on them matching. Only zero_sudo:test_07_common_setup_class_sql
        # (this project's own coverage anchor for this exact statement) reads 'web.base.url'
        # back via a real, separate connection, and it tolerates either value already.
        with odoo.sql_db.db_connect(get_db_name()).cursor() as cr:
            cr.execute(  # audit-ignore-sql: # Tested by [@ANCHOR: zero_sudo:COMM_test_common_setup_class_sql] # fmt: skip
                "INSERT INTO ir_config_parameter (key, value) VALUES "
                "('web.base.url', 'https://hams.com'), "
                "('web.base.url.freeze', '1') "
                "ON CONFLICT (key) DO UPDATE SET value=EXCLUDED.value"
            )
            # The context manager automatically commits if no exception is
            # raised -- and this time it is a real cursor, so it is a real
            # commit.
        super().setUpClass()
        # web.base.url.freeze prevents a real, documented Odoo mechanism
        # (res_users.py's admin-login handler: on a successful
        # base.group_system login carrying a base_location, it silently
        # calls ICP.set_param('web.base.url', base_location) unless this
        # freeze param is set) from letting some later admin-login test
        # anywhere in the same registry's lifetime overwrite web.base.url
        # out from under every other test. ir_config_parameter.py's
        # _get_param() is @ormcache('key', cache='stable'), and
        # Registry.clear_cache() with NO arguments defaults to
        # cache_names=('default',) -- which per registry.py's own
        # _CACHES_BY_KEY table does NOT include 'stable'. A bare
        # clear_cache() (what this call used to be, and what common.py's two
        # equivalent inserts still call) never invalidates the 'stable'
        # bucket _get_param lives in, so the freeze row above could sit
        # correctly in the DB while a stale cached lookup kept seeing it as
        # unset. Passing 'stable' explicitly clears it (plus 'default'/
        # 'templates.cached_values', which 'stable' depends on per that same
        # table).
        cls.registry.clear_cache('stable')

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
        # [@ANCHOR: zero_sudo:COMM_cursor_hijacking]
        # ---
                # ---
        # # Verified by [@ANCHOR: zero_sudo:COMM_test_cursor_hijacking]
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
        self.cr.execute("SELECT id FROM res_users WHERE login = 'admin'")  # audit-ignore-sql: # Tested by [@ANCHOR: zero_sudo:COMM_test_admin_user_fetch]
        row = self.cr.fetchone()
        admin_id = row[0] if row else 2
        self.env = odoo.api.Environment(self.cr, admin_id, {})

        # 2. Snapshot exact table counts
        # [@ANCHOR: zero_sudo:COMM_leak_snapshotting]
        # ---
                # ---
        # # Verified by [@ANCHOR: zero_sudo:COMM_test_leak_snapshotting]
        # ---
        self.cr.execute(  # audit-ignore-sql: # Tested by [@ANCHOR: zero_sudo:COMM_test_leak_snapshotting] # fmt: skip
            "SELECT table_name FROM information_schema.tables WHERE table_schema = 'public' AND table_name NOT LIKE 'pg_stat_statements%'"
        )
        self._tables = [r[0] for r in self.cr.fetchall()]
        self._initial_counts = {}
        if self._tables:
            query_parts = []
            for t in self._tables:
                query_parts.append(sql.SQL("SELECT {}, count(1) FROM {}").format(sql.Literal(t), sql.Identifier(t)))
            
            union_query = sql.SQL(" UNION ALL ").join(query_parts)
            self.cr.execute(union_query)  # audit-ignore-sql: # Tested by [@ANCHOR: zero_sudo:COMM_test_leak_snapshotting] # fmt: skip
            for row in self.cr.fetchall():
                self._initial_counts[row[0]] = row[1]

        self._tracked_records = collections.defaultdict(set)

        # 3. Instrument ORM Creation
        # [@ANCHOR: zero_sudo:COMM_orm_instrumentation]
        # ---
                # ---
        # # Verified by [@ANCHOR: zero_sudo:COMM_test_orm_instrumentation]

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
                        # ---
            # # Verified by [@ANCHOR: zero_sudo:COMM_test_automated_cleanup]
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
                                    records.with_user(2).unlink()
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
                                try:
                                    # bug-hunt (2026-09-13): this raw SQL DELETE used to
                                    # run with no savepoint of its own. When it failed
                                    # too (e.g. still FK-blocked), the resulting
                                    # psycopg2 error left the WHOLE cursor's transaction
                                    # aborted for the rest of this teardown -- every
                                    # OTHER tracked model still queued for cleanup in
                                    # this same last-attempt pass would then also fail
                                    # (via a confusing InFailedSqlTransaction rather
                                    # than its own real error), and the eventual
                                    # self.env.cr.commit() below would silently discard
                                    # even successfully-deleted records from earlier in
                                    # this same pass, misreporting them as leaked. A
                                    # savepoint here contains one model's fallback
                                    # failure to itself, matching the ORM unlink
                                    # attempt's own savepoint just above.
                                    table = model_env._table
                                    with self.env.cr.savepoint():
                                        self.env.cr.execute(
                                            sql.SQL("DELETE FROM {} WHERE id IN %s").format(sql.Identifier(table)),
                                            (tuple(ids),)
                                        )
                                    _logger.info("SQL fallback succeeded for %s %s", model_name, ids)
                                    pending_deletes = False
                                    self._tracked_records[model_name] = set()
                                except Exception as sql_e:  # audit-ignore-catch-all
                                    _logger.error("SQL fallback also failed for %s %s: %s", model_name, ids, sql_e)
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
                        # ---
            # # Verified by [@ANCHOR: zero_sudo:COMM_test_leak_verification]
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

            if self._tables:
                tables_to_check = [t for t in self._tables if t not in noisy_tables]
                if tables_to_check:
                    query_parts = []
                    for t in tables_to_check:
                        query_parts.append(sql.SQL("SELECT {}, count(1) FROM {}").format(sql.Literal(t), sql.Identifier(t)))
                    
                    union_query = sql.SQL(" UNION ALL ").join(query_parts)
                    self.cr.execute(union_query)  # audit-ignore-sql: # Tested by [@ANCHOR: zero_sudo:COMM_test_automated_cleanup] # fmt: skip
                    for row in self.cr.fetchall():
                        t_name, final_count = row
                        initial_count = self._initial_counts.get(t_name, 0)
                        diff = final_count - initial_count
                        if diff != 0:
                            leaks.append(f"{t_name} ({diff:+d})")

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
            # [@ANCHOR: zero_sudo:COMM_test_cursor_restoration]
            # ---
            # # Verified by [@ANCHOR: zero_sudo:COMM_test_cursor_restoration]
            self.registry.cursor = self._test_cursor
            self.cr = self._test_cursor
            self.env = self._test_env
