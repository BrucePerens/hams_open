# -*- coding: utf-8 -*-
from . import real_transaction
from . import test_facility
from . import test_ui
from . import test_integration

import logging
import psycopg2
import odoo.sql_db

_logger = logging.getLogger(__name__)

# ====================================================================================
# THE CURSOR MONITOR TRAP
# ====================================================================================
# Dynamically monkey-patches Odoo's native TestCursor to detect transaction impedance.
# If a test requires RealTransactionCase but is using a standard HttpCase/TransactionCase,
# the resulting thread-locking or savepoint-bypass will throw specific psycopg2 errors.
# We catch these and emit a highly visible AI/Developer hint to instantly halt debugging.

if hasattr(odoo.sql_db, 'TestCursor'):
    _original_test_cursor_execute = odoo.sql_db.TestCursor.execute

    def _monitored_test_execute(self, query, params=None):
        try:
            return _original_test_cursor_execute(self, query, params)
        except psycopg2.Error as e:
            # 40001: SerializationFailure (Concurrent Update Deadlock)
            # 55P03: LockNotAvailable (Row-level lock held by another thread)
            # 23505: UniqueViolation (Often happens when raw SQL bypasses the savepoint)
            if getattr(e, 'pgcode', None) in ('40001', '55P03', '23505'):
                hint = (
                    "\n" + "=" * 80 + "\n"
                    "🚨 [FRAMEWORK HINT] TRANSACTION IMPEDANCE DETECTED 🚨\n"
                    f"A PostgreSQL conflict ({e.pgcode}) occurred inside the isolated TestCursor.\n"
                    "This almost always means a background thread, daemon, or raw SQL bypassed\n"
                    "the uncommitted test savepoint, creating an invisible data collision.\n\n"
                    "💡 FIX: Stop debugging the logic. Convert this test class to inherit from\n"
                    "         `hams_test.tests.real_transaction.RealTransactionCase`.\n"
                    + "=" * 80 + "\n"
                )
                # Emit directly to the logger so the FailureExtractor catches it instantly
                _logger.critical(hint)
            raise

    odoo.sql_db.TestCursor.execute = _monitored_test_execute
